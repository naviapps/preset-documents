# ``PresetDocuments``

Versioned JSON preset documents validated by a shared Rust core.

The SDK owns document format/version/metadata and limits. Hosts own payload
schemas, migrations and import policy, file selection, clipboard access, storage policy and UI.
Input is strictly decoded from saved UTF-8/16/32 JSON and output is UTF-8 JSON.
Limits apply to original bytes and normalized output. Known malformed metadata is
an invalid document; payload decoder errors cannot impersonate format errors.
Envelopes, `formatVersion` and non-null `metadata` must be JSON objects. Positional
arrays are rejected for those structures; payload arrays remain valid.
The Rust encoder stops retaining output once the byte limit is exceeded. It
continues counting to report the full encoded size in `outputTooLarge`.
Swift and .NET validate limits through Rust before invoking host payload encoders.
Invalid limits produce `invalidConfiguration` without visiting the payload.
Typed collection writes also check the full document count before encoding a
payload. The `tooManyDocuments` error reports that count.
Swift `PresetFormat` uses the same Rust validation for explicit construction and
`Codable` decoding. Invalid identifiers or version ranges report `invalidConfiguration`.
For exchange with other bindings, use `PresetFormat.encodeJSON()` and
`PresetFormat.init(json:)`. Rust validates the original UTF-8 JSON object,
including duplicate, unknown and missing fields.
Rust compacts string escapes in completed typed writes before checking their
input budget. New JSON uses literal slashes, punctuation and supplementary
Unicode characters. The original transport to Rust is capped at 64 MiB. Saved
input is validated as supplied.
Read `PresetMetadata.createdAtRFC3339` to retain the timestamp text returned by
Rust, including nanosecond fractions and offsets. The `createdAt` date remains
available at Foundation's precision and uses its historical calendar mapping.
To supply timestamp text, initialize metadata with `createdAtRFC3339`. Rust
applies the same timestamp rules when projecting text into a date and saving
the metadata.
Swift result `Data` owns its bytes independently of later native calls or file changes.
Its storage releases transferred native buffers when it no longer needs their bytes;
empty results release their native buffer immediately.

Collection reads reject the first element beyond `maximumDocumentCount` before
materializing its envelope and opaque payload. The `tooManyDocuments` error
reports that first excess count (the limit plus one), not the total collection
length; excess elements are not validated. Byte, Unicode/string and depth checks
inspect the whole input, including decoding strings, before collection parsing.

The codec itself does no file I/O. The Swift products share one native framework
containing both the codec and file adapter; selecting the codec alone does not
remove native filesystem dependencies. Rust callers can select the core crate
independently. Hosts choose import policy and file paths.

Use `PresetJSONCodec.normalizeDocumentEnvelope(from:matching:limits:)` for a
single document and `PresetJSONCodec.normalizeDocumentEnvelopes(from:matching:limits:)`
for a collection. These methods validate the selected root and normalize its
envelopes to UTF-8 JSON. Payload lexemes, including number spelling, string
escapes and internal whitespace, remain exact. Hosts own payload migration.

```swift
import PresetDocuments

struct Layout: Codable { let columns: Int }
let format = try PresetFormat(identifier: "com.example.layout")
let document = format.makeDocument(payload: Layout(columns: 2))
let data = try PresetJSONCodec.encode(document)
let restored = try PresetJSONCodec.decodeDocument(Layout.self, from: data, matching: format)
```

## Topics

### Documents and formats

- ``PresetFormat``
- ``PresetFormatVersion``
- ``PresetDocument``
- ``PresetMetadata``

### JSON and validation

- ``PresetJSONCodec``
- ``PresetDocumentLimits``
- ``PresetDocumentError``
