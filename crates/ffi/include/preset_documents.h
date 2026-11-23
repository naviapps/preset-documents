#ifndef PRESET_DOCUMENTS_H
#define PRESET_DOCUMENTS_H
#define PRESET_DOCUMENTS_ABI_VERSION 2U
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct PresetDocumentsBuffer PresetDocumentsBuffer;
typedef struct PresetDocumentsTimestampParts {
    double whole_seconds;
    uint8_t fraction[9];
    uint8_t fraction_length;
} PresetDocumentsTimestampParts;
uint32_t preset_documents_abi_version(void);
/* Parts retain the caller's epoch. Seconds are rounded to a nanosecond, carrying
 * to whole_seconds when needed. The ASCII fraction has no decimal point or
 * trailing zeros. Non-finite inputs are returned unchanged with no fraction. */
PresetDocumentsTimestampParts preset_documents_timestamp_parts(double seconds);
/* Optional file operations: 5 reads bounded bytes (empty data); 6 atomically
 * writes with replacement; 7 atomically creates without replacement.
 * Options: {path:absoluteUTF8Path,maximumByteCount:1..64MiB}.
 * Read requires a regular file (symbolic links are followed), rejects Unix FIFOs
 * without waiting for a writer, and observes the stream budget, including growth.
 * Write success means a committed file. No parent directories are created.
 * Operation 6 creates an initially absent destination exclusively. A concurrent
 * creator results in fileAccess with kind DestinationChanged. On a platform or
 * volume without exclusive creation it returns atomicNewUnsupported for new files.
 * Operation 7 reports destinationAlreadyExists on collision or atomicNewUnsupported
 * when the platform or filesystem lacks atomic creation without replacement.
 * Operation 10 checks write size before a caller copies its input. Pass empty
 * data and {byteCount,maximumByteCount} options. Success returns empty bytes;
 * excess size is outputTooLarge. This checks only the budget and does no file
 * I/O. It requires a matching native library. Operations 6, 7 and 9 still
 * validate the actual submitted payload. */
/* Approved replacement (local APFS on macOS only): operation 8 takes empty data
 * and {path:absoluteUTF8Path} options. Keep the returned opaque revision bytes
 * unchanged for this approval flow; it is not a persistent lock or permission.
 * preset_documents_replace_approved takes the replacement and revision bytes
 * separately. The revision supplies the destination path, and maximum_byte_count
 * is in 1..64MiB. The legacy operation 9 remains available with replacement
 * bytes and {path,maximumByteCount,revision} options, with the path required
 * to match the captured path exactly.
 * A stale target observed by the precommit check reports destinationChanged
 * without attempting an exchange.
 * Unsupported platforms or volumes report approvedReplacementUnsupported.
 * This operation never falls back to creating an absent destination. It is not
 * an atomic compare-and-swap: a final concurrent substitution may be displaced.
 * Replacement success JSON is {state:"committed",recoveryDirectory,displacedIdentity}
 * or {state:"commitUncertain",recoveryDirectory}. In the latter case, inspect the
 * destination and recovery directory; do not blindly retry. After a confirmed
 * exchange the directory retains the displaced entry named displaced-preset.
 * recoveryDirectory is its last observed path; the item is not an immutable copy.
 * displacedIdentity is matchesApprovedFile, different, or unverified. A match
 * identifies the object, not its exact pre-exchange content or revision.
 * The caller owns recovery handling; do not automatically discard the entry. */
/* Operation 1 validates a format. Operation 12 validates a portable format
 * JSON object with identifier, minimumSupportedVersion and currentVersion.
 * It rejects duplicate, unknown or missing fields and returns the canonical
 * format JSON object. Operation 12 takes UTF-8 data with no options.
 * Operation 2 (normalizeDocumentEnvelopes)
 * validates documents against a required format and normalizes only envelopes
 * to UTF-8 JSON. Payload lexemes, including numbers, escapes and internal
 * whitespace, are preserved; this is not whole-document canonicalization.
 * Operation 3 writes documents. Both use an explicit document/collection root.
 * Operation 4 validates Limits JSON supplied in data and returns canonical
 * validated limits JSON on success. Operations 1 and 4 do not use options;
 * pass NULL and zero. Validate limits before invoking host payload encoders.
 * Operation 13 checks limits and the full typed collection count before host
 * encoding. Pass {limits,documentCount} as UTF-8 JSON data and no options.
 * This operation requires a matching native library and returns empty bytes
 * on success. Saved JSON still goes through its normal validation.
 * Envelopes, formatVersion and non-null metadata must be JSON objects;
 * positional arrays are rejected, while payload arrays remain valid.
 * Both document operations enforce collection count limits before decoding the
 * first excess element; tooManyDocuments.documentCount is the limit plus one,
 * not the total array length. Whole-input byte, Unicode/string and depth checks
 * run first. Excess elements are not validated after the count limit is reached.
 * Inputs are borrowed for this call. Nonzero lengths require readable pointers.
 * Result owners must be freed once; status 0 means result bytes, 1 means error JSON. */
/* Operation 11 writes completed UTF-8 JSON from a typed serializer. It uses the
 * same options as operation 3 and compacts the escapes in JSON string tokens
 * before applying maximumByteCount. Other payload lexemes remain unchanged.
 * The original transport remains subject to the 64 MiB dispatcher limit.
 * Operation 11 requires a matching native library. */
/* Operation 14 projects raw UTF-8 RFC3339 text with no options. Success is JSON
 * {canonical,unixSeconds,nanoseconds,offsetSeconds,hasSubnanosecondPrecision}.
 * unixSeconds is the whole second rounded toward negative infinity. The fraction
 * is in 0..999999999. The precision flag records discarded nonzero digits.
 * Invalid timestamp text reports invalidDocument. Operation 14 requires a
 * matching native library and leaves the input text unchanged. */
PresetDocumentsBuffer *preset_documents_process(uint32_t operation, const uint8_t *data,
                                                size_t length, const uint8_t *options,
                                                size_t options_length);
PresetDocumentsBuffer *preset_documents_replace_approved(const uint8_t *data, size_t length,
                                                         const uint8_t *revision,
                                                         size_t revision_length,
                                                         size_t maximum_byte_count);
const uint8_t *preset_documents_buffer_data(const PresetDocumentsBuffer *owner, size_t *length,
                                            uint32_t *status);
void preset_documents_buffer_free(PresetDocumentsBuffer *owner);
#ifdef __cplusplus
}
#endif
#endif
