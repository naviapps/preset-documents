//! Owned result buffers. No borrowed native byte view may outlive its buffer.
use preset_documents::{Error, Format, Limits, Root};
use preset_documents_file_io::{
    ReplacementRevision, capture_replacement, replace_approved, validate_write_size,
};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::path::Path;

const MAXIMUM_OPTIONS_BYTES: usize = 16 * 1024;

/// Native ABI version returned by `preset_documents_abi_version`.
pub const PRESET_DOCUMENTS_ABI_VERSION: u32 = 2u32;

/// Owns the result bytes and their status. Release with `preset_documents_buffer_free`.
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

/// Parts retain the caller's epoch. Seconds are rounded to a nanosecond and carry
/// to `whole_seconds` when needed. The ASCII fraction omits the decimal point and
/// trailing zeros. Non-finite inputs are returned unchanged with no fraction.
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
    PRESET_DOCUMENTS_ABI_VERSION
}

fn apply_replacement<'a>(
    path: &Path,
    revision: &ReplacementRevision,
    maximum_byte_count: usize,
    length: usize,
    borrow_data: impl FnOnce() -> &'a [u8],
) -> Result<Vec<u8>, Error> {
    if !cfg!(target_os = "macos") {
        return Err(Error::ApprovedReplacementUnsupported);
    }
    validate_write_size(length, maximum_byte_count)?;
    let outcome = replace_approved(path, revision, borrow_data(), maximum_byte_count)?;
    serde_json::to_vec(&outcome).map_err(|_| Error::NativeFailure)
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
        return apply_replacement(
            path,
            &revision,
            options.maximum_byte_count,
            length,
            borrow_data,
        );
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

/// Dispatches a document, format, timestamp or file operation. Free each result
/// owner once. Status 0 denotes successful result bytes and status 1 error JSON.
///
/// Operation 1 validates a format. Operation 12 validates a portable format JSON
/// object with `identifier`, `minimumSupportedVersion` and `currentVersion`. It
/// rejects duplicate, unknown or missing fields and returns canonical format JSON.
/// Supply UTF-8 data and no options to operation 12.
///
/// Operation 2 validates documents against a required format and normalizes the
/// envelopes to UTF-8 JSON. Payload number spelling, escapes and internal whitespace
/// are preserved. Operation 3 writes documents. Both require an explicit `document`
/// or `collection` root. Envelopes, `formatVersion` and non-null `metadata` must
/// be JSON objects. Payload arrays are valid. Both operations enforce collection
/// count limits before decoding the first excess element. `tooManyDocuments.documentCount`
/// reports the limit plus one. Whole-input byte, Unicode/string and depth checks
/// run first. Excess elements are left unvalidated once the count limit is reached.
///
/// Operation 4 returns the canonical validated `Limits` JSON from `data`. For
/// operations 1 and 4, pass NULL and zero for options. Validate limits before
/// invoking host payload encoders. Operation 13 checks limits and the full typed
/// collection count before host encoding. Pass `{limits,documentCount}` as UTF-8
/// data and no options. Success returns empty bytes. Saved JSON still goes through
/// its normal validation. Operation 13 requires a matching native library.
///
/// Operation 11 writes completed UTF-8 JSON from a typed serializer. It takes
/// the same options as operation 3 and compacts JSON string token escapes before
/// applying `maximumByteCount`. Other payload lexemes are unchanged. The original
/// transport has a separate 64 MiB dispatcher limit. Use a matching native library.
///
/// Operation 14 projects raw UTF-8 RFC3339 text with no options. Success returns
/// `{canonical,unixSeconds,nanoseconds,offsetSeconds,hasSubnanosecondPrecision}`.
/// `unixSeconds` is rounded toward negative infinity. The fraction is 0..999999999
/// and the flag records discarded nonzero digits. Invalid timestamp text reports
/// `invalidDocument`. Use a matching native library. The input text is unchanged.
///
/// Optional file operations 5, 6 and 7 take `{path,maximumByteCount}` options.
/// Use an absolute UTF-8 path and a byte limit from 1 through 64 MiB. Operation 5
/// reads bounded bytes with empty data. It follows symlinks to regular files,
/// rejects Unix FIFOs without waiting for a writer and enforces the stream budget
/// as the file grows. Operation 6 atomically writes with replacement and creates an
/// initially absent destination exclusively. A concurrent creator reports `fileAccess`
/// with kind `DestinationChanged`. Operation 7 creates without replacement and
/// reports `destinationAlreadyExists` on collision. Both report `atomicNewUnsupported`
/// when the platform or volume lacks exclusive creation for a new file. A successful
/// write is committed. Parent directories are never created.
///
/// Operation 10 checks the write budget without file I/O. Pass empty data and
/// `{byteCount,maximumByteCount}` options. It returns empty bytes on success and
/// `outputTooLarge` when the budget is exceeded. Use a matching native library.
/// Operations 6, 7 and 9 still validate the submitted payload.
///
/// Operation 8 captures a revision for approved replacement on local macOS APFS.
/// Pass empty data and `{path}` options with an absolute UTF-8 path. Keep the
/// opaque result bytes unchanged for the approval flow. They supply neither a
/// persistent lock nor permission. Pass the revision to `preset_documents_replace_approved`.
/// Legacy operation 9 takes replacement bytes and `{path,maximumByteCount,revision}`
/// options. Its path must match the captured path exactly. See
/// `preset_documents_replace_approved` for replacement outcomes and recovery.
///
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
    owned_result(|| {
        if !matches!(operation, 6 | 7 | 9) && length > preset_documents::MAXIMUM_BYTES {
            return Err(Error::InputTooLarge {
                byte_count: length,
                maximum_byte_count: preset_documents::MAXIMUM_BYTES,
            });
        }
        if options_length > MAXIMUM_OPTIONS_BYTES
            || (data.is_null() && length != 0)
            || (options.is_null() && options_length != 0)
        {
            return Err(Error::InvalidConfiguration);
        }
        // File writes validate their destination budget before borrowing these bytes.
        process(
            operation,
            unsafe { input_bytes(options, options_length) },
            length,
            || unsafe { input_bytes(data, length) },
        )
    })
}

/// Replaces the exact path in the revision bytes returned by operation 8. Pass a
/// separate replacement payload and a `maximum_byte_count` from 1 through 64 MiB.
///
/// Approved replacement requires local macOS APFS. Other platforms and volumes
/// report `approvedReplacementUnsupported`. A stale target observed by the
/// precommit check reports `destinationChanged` before any exchange. This operation
/// never creates an absent destination. A final concurrent substitution may be
/// displaced because the revision check and entry exchange are separate operations.
///
/// Success is `{state:"committed",recoveryDirectory,displacedIdentity}` or
/// `{state:"commitUncertain",recoveryDirectory}`. When commit is uncertain, inspect
/// the destination and recovery directory before retrying. A confirmed exchange
/// retains the displaced entry as `displaced-preset` in the recovery directory.
/// `recoveryDirectory` gives its last observed path. The entry can still change.
/// `displacedIdentity` is `matchesApprovedFile`, `different` or `unverified`. A
/// match identifies the object only. The bytes may have changed after approval.
/// The caller owns recovery handling. Do not automatically discard the entry.
///
/// # Safety
/// Inputs must be readable and unchanged for their lengths during this call.
/// Null is accepted only for empty data. Free the returned owner exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn preset_documents_replace_approved(
    data: *const u8,
    length: usize,
    revision: *const u8,
    revision_length: usize,
    maximum_byte_count: usize,
) -> *mut Buffer {
    owned_result(|| {
        if revision.is_null()
            || revision_length == 0
            || revision_length > MAXIMUM_OPTIONS_BYTES
            || (data.is_null() && length != 0)
        {
            return Err(Error::InvalidConfiguration);
        }
        let revision: ReplacementRevision =
            configuration_object(unsafe { input_bytes(revision, revision_length) })?;
        let path = replacement_path(revision.captured_path())?;
        apply_replacement(path, &revision, maximum_byte_count, length, || unsafe {
            input_bytes(data, length)
        })
    })
}

/// A nonempty input must remain readable and immutable for the lifetime of the return value.
unsafe fn input_bytes<'a>(data: *const u8, length: usize) -> &'a [u8] {
    if length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, length) }
    }
}

fn owned_result(
    operation: impl FnOnce() -> Result<Vec<u8>, Error> + std::panic::UnwindSafe,
) -> *mut Buffer {
    let result = std::panic::catch_unwind(operation).unwrap_or(Err(Error::NativeFailure));
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

/// Returns a byte view borrowed from `owner`, with status 0 for success and 1 for error JSON.
/// Returns null if any argument is null. Otherwise the view is valid until the owner is freed.
///
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
mod portable_wire_tests;

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
