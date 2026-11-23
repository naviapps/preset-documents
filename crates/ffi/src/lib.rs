//! Owned result buffers. No borrowed native byte view may outlive its buffer.
use preset_documents::{Error, Format, Limits, Root};
use preset_documents_file_io::{
    ReplacementRevision, capture_replacement, replace_approved, validate_write_size,
};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::path::Path;

pub struct Buffer {
    status: u32,
    bytes: Vec<u8>,
}

/// A floating-point timestamp split at nanosecond precision without changing its epoch.
#[repr(C)]
pub struct PresetDocumentsTimestampParts {
    pub whole_seconds: f64,
    pub fraction: [u8; 9],
    pub fraction_length: u8,
}

/// Leaves calendar conversion to the host and returns the significant fractional digits.
#[unsafe(no_mangle)]
pub extern "C" fn preset_documents_timestamp_parts(seconds: f64) -> PresetDocumentsTimestampParts {
    let mut parts = PresetDocumentsTimestampParts {
        whole_seconds: seconds,
        fraction: [b'0'; 9],
        fraction_length: 0,
    };
    if !seconds.is_finite() {
        return parts;
    }
    let whole = seconds.floor();
    let mut nanoseconds = ((seconds - whole) * 1_000_000_000.0).round() as u32;
    parts.whole_seconds = whole;
    if nanoseconds == 1_000_000_000 {
        parts.whole_seconds = whole + 1.0;
        return parts;
    }
    for digit in parts.fraction.iter_mut().rev() {
        *digit += (nanoseconds % 10) as u8;
        nanoseconds /= 10;
    }
    parts.fraction_length = parts
        .fraction
        .iter()
        .rposition(|digit| *digit != b'0')
        .map_or(0, |index| (index + 1) as u8);
    parts
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Options {
    format: Option<Format>,
    limits: Limits,
    root: Root,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureReplacementOptions {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplaceApprovedOptions<'a> {
    path: String,
    maximum_byte_count: usize,
    #[serde(borrow)]
    revision: &'a RawValue,
}

fn configuration_object<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, Error> {
    // A derived struct deserializer may also accept a positional sequence.
    if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        != Some(b'{')
    {
        return Err(Error::InvalidConfiguration);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::InvalidConfiguration)
}

fn replacement_path(value: &str) -> Result<&Path, Error> {
    let path = Path::new(value);
    if !path.is_absolute() || value.contains('\0') {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path)
}

#[unsafe(no_mangle)]
pub extern "C" fn preset_documents_abi_version() -> u32 {
    1
}

fn process<'a>(
    operation: u32,
    options: &[u8],
    length: usize,
    borrow_data: impl FnOnce() -> &'a [u8],
) -> Result<Vec<u8>, Error> {
    if operation == 8 {
        if length != 0 {
            return Err(Error::InvalidConfiguration);
        }
        let options: CaptureReplacementOptions = configuration_object(options)?;
        let revision = capture_replacement(replacement_path(&options.path)?)?;
        return serde_json::to_vec(&revision).map_err(|_| Error::NativeFailure);
    }
    if operation == 9 {
        let options: ReplaceApprovedOptions<'_> = configuration_object(options)?;
        let path = replacement_path(&options.path)?;
        let revision: ReplacementRevision =
            configuration_object(options.revision.get().as_bytes())?;
        if !cfg!(target_os = "macos") {
            return Err(Error::ApprovedReplacementUnsupported);
        }
        validate_write_size(length, options.maximum_byte_count)?;
        let outcome = replace_approved(path, &revision, borrow_data(), options.maximum_byte_count)?;
        return serde_json::to_vec(&outcome).map_err(|_| Error::NativeFailure);
    }

    if operation == 10 {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct WriteBudget {
            byte_count: usize,
            maximum_byte_count: usize,
        }
        if length != 0 {
            return Err(Error::InvalidConfiguration);
        }
        let options: WriteBudget = configuration_object(options)?;
        validate_write_size(options.byte_count, options.maximum_byte_count)?;
        return Ok(Vec::new());
    }

    if (5..=7).contains(&operation) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct FileOptions {
            path: String,
            maximum_byte_count: usize,
        }
        let options: FileOptions =
            serde_json::from_slice(options).map_err(|_| Error::InvalidConfiguration)?;
        let path = Path::new(&options.path);
        if !path.is_absolute() {
            return Err(Error::InvalidConfiguration);
        }
        if operation == 5 {
            return preset_documents_file_io::read(path, options.maximum_byte_count);
        }
        validate_write_size(length, options.maximum_byte_count)?;
        let data = borrow_data();
        return if operation == 7 {
            preset_documents_file_io::write_new(path, data, options.maximum_byte_count)
                .map(|_| Vec::new())
        } else {
            preset_documents_file_io::write_atomically(path, data, options.maximum_byte_count)
                .map(|_| Vec::new())
        };
    }

    if operation == 14 {
        if !options.is_empty() {
            return Err(Error::InvalidConfiguration);
        }
        let text = std::str::from_utf8(borrow_data()).map_err(|_| Error::InvalidDocument)?;
        let projection = preset_documents::project_metadata_timestamp(text)?;
        return serde_json::to_vec(&projection).map_err(|_| Error::NativeFailure);
    }

    let data = borrow_data();
    if matches!(operation, 1 | 12) {
        let format: Format = if operation == 12 {
            if !options.is_empty() {
                return Err(Error::InvalidConfiguration);
            }
            configuration_object(data)?
        } else {
            serde_json::from_slice(data).map_err(|_| Error::InvalidConfiguration)?
        };
        format.validate()?;
        return serde_json::to_vec(&format).map_err(|_| Error::NativeFailure);
    }
    if operation == 4 {
        let limits: Limits =
            serde_json::from_slice(data).map_err(|_| Error::InvalidConfiguration)?;
        limits.validate()?;
        return serde_json::to_vec(&limits).map_err(|_| Error::NativeFailure);
    }
    if operation == 13 {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct TypedCollection<'a> {
            #[serde(borrow)]
            limits: &'a RawValue,
            document_count: usize,
        }
        if !options.is_empty() {
            return Err(Error::InvalidConfiguration);
        }
        let collection: TypedCollection<'_> = configuration_object(data)?;
        let limits: Limits = configuration_object(collection.limits.get().as_bytes())?;
        limits.validate_document_count(collection.document_count)?;
        return Ok(Vec::new());
    }
    let options: Options =
        serde_json::from_slice(options).map_err(|_| Error::InvalidConfiguration)?;
    if !matches!(operation, 2 | 3 | 11) {
        return Err(Error::InvalidConfiguration);
    }
    if operation == 2 {
        let format = options.format.as_ref().ok_or(Error::InvalidConfiguration)?;
        return preset_documents::normalize_document_envelopes(
            data,
            format,
            &options.limits,
            options.root,
        );
    }
    if operation == 11 {
        return preset_documents::encode_typed_documents(
            data,
            options.format.as_ref(),
            &options.limits,
            options.root,
        );
    }
    let documents =
        preset_documents::decode(data, options.format.as_ref(), &options.limits, options.root)?;
    preset_documents::encode(&documents, &options.limits, options.root)
}

/// # Safety
/// Inputs must be readable for their lengths and remain unchanged during this
/// call. Null is accepted only with zero length. Length checks cannot validate
/// arbitrary foreign pointers. The returned owner must be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn preset_documents_process(
    operation: u32,
    data: *const u8,
    length: usize,
    options: *const u8,
    options_length: usize,
) -> *mut Buffer {
    let result = std::panic::catch_unwind(|| {
        if !matches!(operation, 6 | 7 | 9) && length > preset_documents::MAXIMUM_BYTES {
            return Err(Error::InputTooLarge {
                byte_count: length,
                maximum_byte_count: preset_documents::MAXIMUM_BYTES,
            });
        }
        if options_length > 16384
            || (data.is_null() && length != 0)
            || (options.is_null() && options_length != 0)
        {
            return Err(Error::InvalidConfiguration);
        }
        let options = if options_length == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(options, options_length) }
        };
        // File writes validate their destination budget before borrowing these bytes.
        process(operation, options, length, || {
            if length == 0 {
                &[]
            } else {
                unsafe { std::slice::from_raw_parts(data, length) }
            }
        })
    })
    .unwrap_or(Err(Error::NativeFailure));
    let buffer = match result {
        Ok(bytes) => Buffer { status: 0, bytes },
        Err(error) => Buffer {
            status: 1,
            bytes: serde_json::to_vec(&error)
                .unwrap_or_else(|_| br#"{"code":"nativeFailure"}"#.to_vec()),
        },
    };
    Box::into_raw(Box::new(buffer))
}

/// # Safety
/// Owner must be live and originate from this SDK; output pointers must be
/// writable. Copy bytes before freeing. Concurrent mutation/free is forbidden.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn preset_documents_buffer_data(
    owner: *const Buffer,
    length: *mut usize,
    status: *mut u32,
) -> *const u8 {
    if owner.is_null() || length.is_null() || status.is_null() {
        return std::ptr::null();
    }
    let value = unsafe { &*owner };
    unsafe {
        *length = value.bytes.len();
        *status = value.status;
    }
    value.bytes.as_ptr()
}

/// # Safety
/// Owner must be null or a live SDK buffer, freed at most once with no readers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn preset_documents_buffer_free(owner: *mut Buffer) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner) });
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, preset_documents_timestamp_parts, process};

    #[test]
    fn typed_collection_checks_full_count_after_validating_limits() {
        for (input, result) in [
            (
                br#"{"limits":{"maximumByteCount":64,"maximumDocumentCount":1,"maximumDepth":8},"documentCount":0}"#.as_slice(),
                Ok(Vec::new()),
            ),
            (
                br#"{"limits":{"maximumByteCount":64,"maximumDocumentCount":1,"maximumDepth":8},"documentCount":1}"#,
                Ok(Vec::new()),
            ),
            (
                br#"{"limits":{"maximumByteCount":64,"maximumDocumentCount":1,"maximumDepth":8},"documentCount":3}"#,
                Err(Error::TooManyDocuments {
                    document_count: 3,
                    maximum_document_count: 1,
                }),
            ),
            (
                br#"{"limits":{"maximumByteCount":0,"maximumDocumentCount":1,"maximumDepth":8},"documentCount":3}"#,
                Err(Error::InvalidConfiguration),
            ),
            (
                br#"{"limits":[64,1,8],"documentCount":1}"#,
                Err(Error::InvalidConfiguration),
            ),
            (
                br#"{"limits":{"maximumByteCount":64,"maximumDocumentCount":1,"maximumDepth":8},"documentCount":-1}"#,
                Err(Error::InvalidConfiguration),
            ),
        ] {
            assert_eq!(process(13, &[], input.len(), || input), result);
        }
    }

    #[test]
    fn portable_format_json_requires_an_unambiguous_object() {
        let canonical =
            br#"{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":3}"#;
        let reordered =
            br#"{ "currentVersion": 3, "identifier": "com.example.presets", "minimumSupportedVersion": 2 }"#;
        assert_eq!(
            process(12, &[], reordered.len(), || reordered),
            Ok(canonical.to_vec())
        );
        assert_eq!(
            process(12, b"{}", canonical.len(), || canonical),
            Err(Error::InvalidConfiguration)
        );

        let positional = br#"["com.example.presets",2,3]"#;
        assert_eq!(
            process(1, &[], positional.len(), || positional),
            Ok(canonical.to_vec())
        );
        for input in [
            positional.as_slice(),
            br#"null"#,
            br#"{"identifier":"com.example.presets","minimumSupportedVersion":2,"minimumSupportedVersion":1,"currentVersion":3}"#,
            br#"{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":3,"future":true}"#,
            br#"{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":1}"#,
            br#"{"Identifier":"com.example.presets","MinimumSupportedVersion":2,"CurrentVersion":3}"#,
            br#"{"identifier":"com.example.presets","currentVersion":3}"#,
            br#"{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":"3"}"#,
            br#"{"identifier":"bad/id","minimumSupportedVersion":2,"currentVersion":3}"#,
        ] {
            assert_eq!(
                process(12, &[], input.len(), || input),
                Err(Error::InvalidConfiguration),
                "{}",
                String::from_utf8_lossy(input)
            );
        }
    }

    #[test]
    fn file_write_budget_refuses_input_and_never_borrows_file_bytes() {
        let budget = br#"{"byteCount":5,"maximumByteCount":5}"#;
        let borrow = || panic!("write budget should not borrow file bytes");
        assert_eq!(process(10, budget, 0, borrow), Ok(Vec::new()));
        assert_eq!(
            process(10, budget, 1, borrow),
            Err(Error::InvalidConfiguration)
        );
        assert_eq!(
            process(10, br#"[5,5]"#, 0, borrow),
            Err(Error::InvalidConfiguration)
        );
    }

    #[test]
    fn timestamp_fraction_retains_precision_and_carries_across_seconds() {
        let cases: &[(f64, f64, &[u8])] = &[
            (0.0, 0.0, b""),
            (-0.0, 0.0, b""),
            (1.0, 1.0, b""),
            (0.125, 0.0, b"125"),
            (-0.125, -1.0, b"875"),
            (0.123_456_789, 0.0, b"123456789"),
            (0.000_000_001, 0.0, b"000000001"),
            (0.000_000_010, 0.0, b"00000001"),
            (0.000_000_000_4, 0.0, b""),
            (0.000_000_000_5, 0.0, b"000000001"),
            (0.999_999_999_4, 0.0, b"999999999"),
            (0.999_999_999_6, 1.0, b""),
            (-0.000_000_000_4, 0.0, b""),
            (-1.000_000_001, -2.0, b"999999999"),
            (1e20, 1e20, b""),
        ];
        for &(seconds, whole, fraction) in cases {
            let parts = preset_documents_timestamp_parts(seconds);
            assert_eq!(parts.whole_seconds, whole, "{seconds}");
            assert_eq!(
                &parts.fraction[..usize::from(parts.fraction_length)],
                fraction,
                "{seconds}"
            );
        }
    }

    #[test]
    fn timestamp_fraction_preserves_nonfinite_inputs() {
        for seconds in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            let parts = preset_documents_timestamp_parts(seconds);
            assert_eq!(parts.whole_seconds.to_bits(), seconds.to_bits());
            assert_eq!(parts.fraction_length, 0);
        }
    }
}
