//! Portable versioned JSON documents. Payload meaning, migrations, persistence,
//! selection and import/application policy belong to the host.
use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor, value::MapAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;
use std::{borrow::Cow, io};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub const MAXIMUM_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Error {
    InvalidConfiguration,
    InvalidDocument,
    FormatMismatch {
        expected: String,
        actual: String,
    },
    UnsupportedFormatVersion {
        version: i64,
        minimum_supported_version: i64,
        maximum_supported_version: i64,
    },
    InputTooLarge {
        byte_count: usize,
        maximum_byte_count: usize,
    },
    OutputTooLarge {
        byte_count: usize,
        maximum_byte_count: usize,
    },
    TooManyDocuments {
        /// On decode, the first excess count (limit + 1), not the total length.
        /// On encode, the length of the supplied document slice.
        document_count: usize,
        maximum_document_count: usize,
    },
    NestingTooDeep {
        maximum_depth: usize,
    },
    FileAccess {
        kind: String,
    },
    NativeFailure,
    DestinationAlreadyExists,
    AtomicNewUnsupported,
    /// The approved replacement target no longer matches its captured revision.
    DestinationChanged,
    /// Approved replacement is not supported on this platform or volume.
    ApprovedReplacementUnsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Format {
    pub identifier: String,
    pub minimum_supported_version: i64,
    pub current_version: i64,
}

fn valid_identifier(value: &str) -> bool {
    value.len() <= 1024
        && value.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

impl Format {
    pub fn validate(&self) -> Result<(), Error> {
        if !valid_identifier(&self.identifier)
            || self.minimum_supported_version < 1
            || self.current_version < self.minimum_supported_version
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
    fn validate_version(&self, value: &FormatVersion) -> Result<(), Error> {
        if value.identifier != self.identifier {
            return Err(Error::FormatMismatch {
                expected: self.identifier.clone(),
                actual: value.identifier.clone(),
            });
        }
        if !(self.minimum_supported_version..=self.current_version).contains(&value.version) {
            return Err(Error::UnsupportedFormatVersion {
                version: value.version,
                minimum_supported_version: self.minimum_supported_version,
                maximum_supported_version: self.current_version,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    pub maximum_byte_count: usize,
    pub maximum_document_count: usize,
    pub maximum_depth: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_byte_count: 16 * 1024 * 1024,
            maximum_document_count: 65_536,
            maximum_depth: 128,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), Error> {
        if self.maximum_byte_count == 0
            || self.maximum_byte_count > MAXIMUM_BYTES
            || self.maximum_document_count == 0
            || !(1..=128).contains(&self.maximum_depth)
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }

    /// Validates the limits and checks a caller-owned collection's full count.
    pub fn validate_document_count(&self, document_count: usize) -> Result<(), Error> {
        self.validate()?;
        if document_count > self.maximum_document_count {
            return Err(Error::TooManyDocuments {
                document_count,
                maximum_document_count: self.maximum_document_count,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Root {
    Document,
    Collection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatVersion {
    pub identifier: String,
    pub version: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub created_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_version: Option<String>,
}

/// The portable creation time and the data a host needs to project its date type.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataTimestampProjection {
    pub canonical: String,
    pub unix_seconds: i64,
    pub nanoseconds: u32,
    pub offset_seconds: i32,
    pub has_subnanosecond_precision: bool,
}

/// Uses the document codec's RFC3339 rules without changing the caller's text.
/// The instant uses the same nanosecond precision as saved metadata.
pub fn project_metadata_timestamp(text: &str) -> Result<MetadataTimestampProjection, Error> {
    let date = OffsetDateTime::parse(text, &Rfc3339).map_err(|_| Error::InvalidDocument)?;
    let canonical = date.format(&Rfc3339).map_err(|_| Error::NativeFailure)?;
    // Parsing accepts further fractional digits and truncates to nine. Report
    // discarded nonzero digits so exact host types can decline the projection.
    let has_subnanosecond_precision = text
        .as_bytes()
        .get(19..)
        .and_then(|tail| tail.strip_prefix(b"."))
        .is_some_and(|fraction| {
            fraction
                .iter()
                .take_while(|digit| digit.is_ascii_digit())
                .skip(9)
                .any(|&digit| digit != b'0')
        });
    Ok(MetadataTimestampProjection {
        canonical,
        unix_seconds: date.unix_timestamp(),
        nanoseconds: date.nanosecond(),
        offset_seconds: date.offset().whole_seconds(),
        has_subnanosecond_precision,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub format_version: FormatVersion,
    pub metadata: Metadata,
    pub payload: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecordedDocument {
    #[serde(deserialize_with = "deserialize_object")]
    format_version: FormatVersion,
    #[serde(default)]
    metadata: Option<Box<RawValue>>,
    payload: Box<RawValue>,
}

#[derive(Deserialize)]
#[serde(transparent, bound(deserialize = "T: Deserialize<'de>"))]
struct Object<T>(#[serde(deserialize_with = "deserialize_object")] T);

// Serde's derived struct visitors also accept positional arrays. Require a map
// at each envelope boundary without parsing the opaque payload into a JSON tree.
fn deserialize_object<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    struct ObjectVisitor<T>(std::marker::PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a JSON object")
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(ObjectVisitor(std::marker::PhantomData))
}

struct BoundedCollection<'a> {
    maximum_document_count: usize,
    exceeded: &'a mut bool,
}

impl<'de> Visitor<'de> for BoundedCollection<'_> {
    type Value = Vec<RecordedDocument>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of preset documents")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut documents = Vec::new();
        while documents.len() < self.maximum_document_count {
            match sequence.next_element::<Object<RecordedDocument>>()? {
                Some(document) => documents.push(document.0),
                None => return Ok(documents),
            }
        }
        // SeqAccess detects the end of the array and invalid separators. If
        // another element exists, reject it before decoding or copying it.
        sequence.next_element_seed(ExcessDocument {
            exceeded: self.exceeded,
        })?;
        Ok(documents)
    }
}

struct ExcessDocument<'a> {
    exceeded: &'a mut bool,
}

impl<'de> DeserializeSeed<'de> for ExcessDocument<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, _deserializer: D) -> Result<(), D::Error> {
        *self.exceeded = true;
        Err(serde::de::Error::custom("document count limit exceeded"))
    }
}

fn decode_collection(
    text: &str,
    maximum_document_count: usize,
) -> Result<Vec<RecordedDocument>, Error> {
    let mut exceeded = false;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let result = deserializer.deserialize_seq(BoundedCollection {
        maximum_document_count,
        exceeded: &mut exceeded,
    });
    if exceeded {
        return Err(Error::TooManyDocuments {
            document_count: maximum_document_count.saturating_add(1),
            maximum_document_count,
        });
    }
    let documents = result.map_err(|_| Error::InvalidDocument)?;
    deserializer.end().map_err(|_| Error::InvalidDocument)?;
    Ok(documents)
}

/// Read saved JSON as UTF-8, UTF-16 or UTF-32. The byte budget applies before
/// transcoding; malformed Unicode is rejected rather than repaired.
fn text(bytes: &[u8]) -> Result<Cow<'_, str>, Error> {
    let (unit, little, skip) = match bytes {
        [0xff, 0xfe, 0, 0, ..] => (4, true, 4),
        [0, 0, 0xfe, 0xff, ..] => (4, false, 4),
        [0xff, 0xfe, ..] => (2, true, 2),
        [0xfe, 0xff, ..] => (2, false, 2),
        [a, 0, 0, 0, ..] if *a != 0 => (4, true, 0),
        [0, 0, 0, a, ..] if *a != 0 => (4, false, 0),
        [a, 0, ..] if *a != 0 => (2, true, 0),
        [0, a, ..] if *a != 0 => (2, false, 0),
        _ => (
            1,
            false,
            if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                3
            } else {
                0
            },
        ),
    };
    let bytes = &bytes[skip..];
    if unit == 1 {
        return std::str::from_utf8(bytes)
            .map(Cow::Borrowed)
            .map_err(|_| Error::InvalidDocument);
    }
    if !bytes.len().is_multiple_of(unit) {
        return Err(Error::InvalidDocument);
    }
    if unit == 2 {
        let units = bytes.as_chunks::<2>().0.iter().map(|b| {
            if little {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            }
        });
        return char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map(Cow::Owned)
            .map_err(|_| Error::InvalidDocument);
    }
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| {
            let value = if little {
                u32::from_le_bytes(*b)
            } else {
                u32::from_be_bytes(*b)
            };
            char::from_u32(value).ok_or(Error::InvalidDocument)
        })
        .collect::<Result<String, _>>()
        .map(Cow::Owned)
}

fn check_strings_and_depth(text: &str, maximum_depth: usize) -> Result<(), Error> {
    inspect_strings_and_depth(text, maximum_depth, |_, _, _| Ok(())).map(|_| ())
}

fn inspect_strings_and_depth(
    text: &str,
    maximum_depth: usize,
    mut escaped_string: impl FnMut(usize, &str, &str) -> Result<(), Error>,
) -> Result<bool, Error> {
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    let mut string_start = 0;
    let mut has_escape = false;
    let mut has_compactable_escape = false;
    for (index, byte) in text.bytes().enumerate() {
        if quoted {
            if escaped {
                has_compactable_escape |= matches!(byte, b'/' | b'u');
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
                has_escape = true;
            } else if byte == b'"' {
                // RawValue intentionally does not decode payload strings. Let
                // Serde validate escapes/surrogate pairs without interpreting
                // payload numbers or constructing a second document tree.
                let literal = &text[string_start..=index];
                if has_escape {
                    let value = serde_json::from_str::<String>(literal)
                        .map_err(|_| Error::InvalidDocument)?;
                    if has_compactable_escape {
                        escaped_string(string_start, literal, &value)?;
                    }
                } else {
                    serde_json::from_str::<&str>(literal).map_err(|_| Error::InvalidDocument)?;
                }
                quoted = false;
            }
        } else {
            match byte {
                b'"' => {
                    quoted = true;
                    string_start = index;
                    has_escape = false;
                    has_compactable_escape = false;
                }
                b'{' | b'[' => {
                    depth += 1;
                    if depth > maximum_depth {
                        return Err(Error::NestingTooDeep { maximum_depth });
                    }
                }
                b'}' | b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(!quoted)
}

/// Metadata and envelopes are validated in Rust. Payload JSON is retained
/// verbatim, including large integers; host payload decoding is a separate step.
/// Collections stop before deserializing the first element beyond the count
/// limit. Byte, Unicode and depth checks still apply to the entire input first.
pub fn decode(
    input: &[u8],
    format: Option<&Format>,
    limits: &Limits,
    root: Root,
) -> Result<Vec<Document>, Error> {
    limits.validate()?;
    if let Some(format) = format {
        format.validate()?;
    }
    if input.len() > limits.maximum_byte_count {
        return Err(Error::InputTooLarge {
            byte_count: input.len(),
            maximum_byte_count: limits.maximum_byte_count,
        });
    }
    let text = text(input)?;
    check_strings_and_depth(&text, limits.maximum_depth)?;
    let recorded: Vec<RecordedDocument> = match root {
        Root::Document => vec![
            serde_json::from_str::<Object<RecordedDocument>>(&text)
                .map_err(|_| Error::InvalidDocument)?
                .0,
        ],
        Root::Collection => decode_collection(&text, limits.maximum_document_count)?,
    };
    for document in &recorded {
        if !valid_identifier(&document.format_version.identifier) {
            return Err(Error::InvalidDocument);
        }
        if let Some(format) = format {
            format.validate_version(&document.format_version)?;
        } else if !valid_identifier(&document.format_version.identifier)
            || document.format_version.version < 1
        {
            return Err(Error::InvalidDocument);
        }
    }
    recorded
        .into_iter()
        .map(|document| {
            Ok(Document {
                format_version: document.format_version,
                metadata: document
                    .metadata
                    .map(|raw| {
                        serde_json::from_str::<Object<Metadata>>(raw.get())
                            .map(|metadata| metadata.0)
                            .map_err(|_| Error::InvalidDocument)
                    })
                    .transpose()?
                    .unwrap_or_default(),
                payload: document.payload,
            })
        })
        .collect()
}

/// Validates documents and normalizes only their envelopes to UTF-8 JSON.
/// Payload JSON retains exact lexemes, including numbers, escapes and internal
/// whitespace. Hosts own payload interpretation and migration; this is not
/// whole-document canonicalization. The root is explicit, with no input guessing.
pub fn normalize_document_envelopes(
    input: &[u8],
    format: &Format,
    limits: &Limits,
    root: Root,
) -> Result<Vec<u8>, Error> {
    let documents = decode(input, Some(format), limits, root)?;
    encode(&documents, limits, root)
}

/// Writes a host serializer's completed UTF-8 JSON using shared compact string
/// escapes before measuring its input budget. Other payload lexemes are retained.
/// The transport itself is limited to `MAXIMUM_BYTES`.
pub fn encode_typed_documents(
    input: &[u8],
    format: Option<&Format>,
    limits: &Limits,
    root: Root,
) -> Result<Vec<u8>, Error> {
    limits.validate()?;
    if let Some(format) = format {
        format.validate()?;
    }
    if input.len() > MAXIMUM_BYTES {
        return Err(Error::InputTooLarge {
            byte_count: input.len(),
            maximum_byte_count: MAXIMUM_BYTES,
        });
    }
    let input = match compact_typed_input(input, limits.maximum_byte_count) {
        // Malformed strings keep the raw path's byte and depth error ordering.
        Err(Error::InvalidDocument) => Cow::Borrowed(input),
        compact => compact?,
    };
    let documents = decode(&input, format, limits, root)?;
    encode(&documents, limits, root)
}

fn compact_typed_input(input: &[u8], maximum: usize) -> Result<Cow<'_, [u8]>, Error> {
    use io::Write;

    // Serde already uses the other short escapes. When the bytes contain no
    // candidate slash or Unicode escape, decode can validate the input once.
    if !input.contains(&b'\\')
        || !input
            .windows(2)
            .any(|pair| matches!(pair, [b'\\', b'/' | b'u']))
    {
        return Ok(Cow::Borrowed(input));
    }
    let text = std::str::from_utf8(input).map_err(|_| Error::InvalidDocument)?;
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        byte_count: 0,
        maximum,
    };
    let mut cursor = 0;
    // No container or payload tree is materialized here. Decode applies the
    // depth and collection limits after measuring the compact input.
    let complete = inspect_strings_and_depth(text, usize::MAX, |start, literal, value| {
        let compact = serde_json::to_vec(value).map_err(|_| Error::NativeFailure)?;
        if compact != literal.as_bytes() {
            output
                .write_all(&input[cursor..start])
                .and_then(|()| output.write_all(&compact))
                .map_err(|_| Error::NativeFailure)?;
            cursor = start + literal.len();
        }
        Ok(())
    })?;
    if !complete || cursor == 0 {
        return Ok(Cow::Borrowed(input));
    }
    output
        .write_all(&input[cursor..])
        .map_err(|_| Error::NativeFailure)?;
    if output.byte_count > maximum {
        return Err(Error::InputTooLarge {
            byte_count: output.byte_count,
            maximum_byte_count: maximum,
        });
    }
    Ok(Cow::Owned(output.bytes))
}

struct BoundedOutput {
    bytes: Vec<u8>,
    byte_count: usize,
    maximum: usize,
}

impl io::Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.byte_count = self.byte_count.saturating_add(bytes.len());
        if self.byte_count > self.maximum {
            // Keep counting for the typed error without retaining rejected output.
            self.bytes = Vec::new();
        } else {
            if self.byte_count > self.bytes.capacity() {
                let capacity = self
                    .byte_count
                    .max(self.bytes.capacity() * 2)
                    .min(self.maximum);
                self.bytes.reserve_exact(capacity - self.bytes.len());
            }
            self.bytes.extend_from_slice(bytes);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Normalizes the envelope to UTF-8 JSON without interpreting payload schemas.
/// Output is checked independently because normalization may change byte length.
pub fn encode(documents: &[Document], limits: &Limits, root: Root) -> Result<Vec<u8>, Error> {
    limits.validate()?;
    for document in documents {
        if !valid_identifier(&document.format_version.identifier)
            || document.format_version.version < 1
        {
            return Err(Error::InvalidDocument);
        }
    }
    limits.validate_document_count(documents.len())?;
    let mut output = BoundedOutput {
        bytes: Vec::new(),
        byte_count: 0,
        maximum: limits.maximum_byte_count,
    };
    match root {
        Root::Document if documents.len() == 1 => serde_json::to_writer(&mut output, &documents[0]),
        Root::Document => return Err(Error::InvalidDocument),
        Root::Collection => serde_json::to_writer(&mut output, documents),
    }
    .map_err(|_| Error::InvalidDocument)?;
    if output.byte_count > output.maximum {
        return Err(Error::OutputTooLarge {
            byte_count: output.byte_count,
            maximum_byte_count: output.maximum,
        });
    }
    check_strings_and_depth(
        std::str::from_utf8(&output.bytes).map_err(|_| Error::InvalidDocument)?,
        limits.maximum_depth,
    )?;
    Ok(output.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn output_budget_releases_buffer_and_counts_remaining_writes() {
        let mut output = BoundedOutput {
            bytes: Vec::new(),
            byte_count: 0,
            maximum: 9,
        };
        output.write_all(b"12345678").unwrap();
        output.write_all(b"9").unwrap();
        assert_eq!(output.bytes, b"123456789");
        assert!(output.bytes.capacity() <= 9);
        output.write_all(&[b'x'; 8192]).unwrap();
        output.write_all(b"tail").unwrap();
        assert_eq!(output.byte_count, 9 + 8192 + 4);
        assert_eq!(output.bytes.capacity(), 0);
    }
}
