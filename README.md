# PresetDocuments

Rust-backed versioned preset documents for Swift, Rust, C and .NET.

Swift sources and tests live in `Sources/` and `Tests/`; Rust crates live in
`crates/`, with C ABI tests in `crates/ffi/tests/c/`. .NET projects use
`bindings/dotnet/src/<project>/` and `bindings/dotnet/tests/<project>/` with their
existing project names. Shared document fixtures live in `fixtures/documents/`,
developer tools in `tools/`, and tracked native frameworks in `Artifacts/`.

## Start here

- [Swift, Rust and C installation](#packages) and [the .NET guide](https://github.com/naviapps/preset-documents/blob/main/bindings/dotnet/README.md).
- Swift API catalogs: [codec](https://github.com/naviapps/preset-documents/blob/main/Sources/PresetDocuments/PresetDocuments.docc/PresetDocuments.md)
  and [file I/O](https://github.com/naviapps/preset-documents/blob/main/Sources/PresetDocumentsFileIO/PresetDocumentsFileIO.docc/PresetDocumentsFileIO.md).
- [Maintainer setup and acceptance](#development-and-acceptance).

Swift products require Swift 6.4 and macOS 26 or later; the tracked framework
contains ARM64 and Intel slices. Rust uses the pinned 1.99.1 toolchain. The .NET 10
NuGet package is built for universal macOS 26+ and Windows x64. Windows package
acceptance uses an owned Windows 11 ARM64 host with x64 .NET and emulation.
See [Windows package acceptance](bindings/dotnet/README.md#windows-package-acceptance)
for the tooling and runtime requirements.

## Boundaries

Rust owns document envelopes, format identity/version checks, metadata validation,
Unicode decoding, input/output budgets, document/depth limits and typed errors.
Payload JSON remains opaque and retains its original number spelling and precision.
Hosts own payload schemas and migrations, action/platform compatibility, import
selection/conflicts, storage policy, UI, file selection and clipboard operations.
The optional file adapter owns bounded reads and atomic file publication. Rust callers
can depend on the core codec without the file adapter's filesystem dependencies.

The document envelope uses `formatVersion`, optional `metadata`, and `payload`.
Envelopes, `formatVersion` and non-null `metadata` must be JSON objects; positional
array representations are rejected. Payloads may contain any valid JSON, including arrays.
Format identifiers contain nonempty dot-separated ASCII alphanumeric, hyphen or
underscore segments, at most 1024 UTF-8 bytes in total. Accepted versions are an
explicit positive inclusive range. Metadata has optional `id`, `title`, `createdAt`
(RFC3339) and `producerVersion`; IDs have no storage or synchronization semantics.
Unknown envelope/metadata fields are ignored. Known duplicate fields, malformed
Unicode/JSON and invalid timestamps are rejected. Payload validation belongs to
the host; its decoder failures must not masquerade as native or format errors.
Swift `PresetMetadata.createdAtRFC3339` and .NET `PresetMetadata.CreatedAtRfc3339`
expose the timestamp text chosen by the Rust codec, preserving nanosecond
precision and offsets. Rust parses both authored and saved timestamp text with
the same rules. Swift maps the canonical timestamp through Foundation's calendar
and date precision. .NET projects the parsed instant and offset when it can
represent the time exactly. The authored text remains available independently of
the date projection. Rust rounds the fractional seconds of a Swift-authored
`Date` before Foundation formats the calendar date. Rust exercises the full
timestamp corpus for single documents and collections. Swift and .NET check
representative inputs through their public codecs and native date types.

JSON output is UTF-8. Saved UTF-8 (including BOM), UTF-16 and UTF-32 documents are
read strictly. Original input and normalized output have separate byte checks;
the supported budget is 1 byte through 64 MiB and nesting is at most 128 levels.
Hosts may impose smaller limits. Single documents and document arrays are explicit
operations. The codec does not guess input roots or repair corrupt input. Default limits are 16 MiB, 65,536 documents and depth 128.
The Rust encoder stops retaining output once the byte limit is exceeded. It
continues counting to report the full encoded size in `outputTooLarge`.
Swift and .NET validate limits in Rust before invoking host payload encoders;
invalid configurations fail before converters or cyclic payloads are visited.
Typed collection writes also check their full count before invoking those encoders
and report that count in `tooManyDocuments`. Rust validates the serialized
collection independently of its claimed count.
Both omit unset envelope metadata fields before measuring typed encode input.
Explicit null fields in a payload still contribute to the byte budget.
Completed typed writes use Rust's compact UTF-8 string escapes for property
names and values before measuring input. Numbers, whitespace and property order
in the serializer's payload are retained. The original serializer transport has
a separate 64 MiB ceiling, so an escaped transport from one binding can reach
that ceiling with a smaller document. Read and envelope-normalization operations
still measure saved input as supplied and retain its payload string escapes.

Collection reads enforce the document count while parsing: the first excess
element is rejected before its envelope and opaque payload are materialized. The
`tooManyDocuments` error's `documentCount` is the first detected excess count
(`maximumDocumentCount + 1`), not the full array length. Later elements are not
validated, so a count error can precede a malformed excess envelope or payload.
Original byte, Unicode/string and depth checks still inspect the whole input
first, including decoding strings; this does not provide streaming file I/O.
Rust `encode` already receives a document slice and reports that slice's full
length for a count error. C read and write operations both parse their input
with the bounded collection reader.

## Optional file I/O

Select the `PresetDocumentsFileIO` Swift product and import it to use
`PresetDocumentFiles.read(from:maximumByteCount:)`,
`writeNew(_:to:maximumByteCount:)` and `writeAtomically(_:to:maximumByteCount:)`.
Rust callers use `read`, `write_new` and `write_atomically` in the independent
`preset-documents-file-io` crate. .NET exposes `ReadAsync`, `WriteNewAsync` and
`WriteAtomicallyAsync` on `PresetDocumentFiles`.

The Swift products separate codec and file I/O APIs. Both use the same native
framework, whose FFI depends on both Rust crates; selecting only `PresetDocuments`
does not remove native filesystem dependencies. .NET likewise exposes both APIs
from one assembly and native library. The codec performs no file I/O.

Paths are explicit and absolute in the language/C adapters. Reads accept regular
files, including symbolic links to regular files, and inspect the opened handle.
Swift URLs must be local file URLs: an empty host or case-insensitive `localhost`,
without a user, password, port, query, fragment or percent-encoded slash.
Swift and .NET reject paths ending in a directory separator or containing a NUL.
.NET rejects paths that cannot be encoded as UTF-8 without replacing characters.
Mounted network volumes remain accessible through their explicit local file paths.
On Unix, opening a FIFO does not wait for a writer before rejecting its file type.
Reads enforce their budget on the actual stream, including growth after metadata
inspection. A file write exceeding its configured byte budget reports
`outputTooLarge` with the submitted size and that budget. Both writes stage a
temporary file beside the destination and sync its contents before publishing.
`writeNew` uses an atomic operation that never replaces an existing destination,
including a directory or symbolic link.
An occupied name reports `destinationAlreadyExists`. If the platform or volume
does not support atomic creation without replacement, it reports
`atomicNewUnsupported`; this includes exFAT on the verified macOS build.
The same restriction applies when `writeAtomically` creates an initially absent
file. An overwrite of an existing regular file uses the ordinary replacement operation.
There is no fallback that overwrites or exposes partial content.
`writeAtomically` intentionally replaces an existing regular file or creates
an absent one. It rejects an observed final symbolic link; select its target
explicitly to replace that file. On macOS, overwrites preserve the existing file's readable
Finder tag attribute (up to 1 MiB) on volumes reporting native extended
attributes. Other attributes are not copied. Missing or unreadable tags due to
permissions or lack of attribute support do not prevent an overwrite. A tag
transfer failure aborts before replacement. For an existing file, all platforms
recheck its existence, size and available timestamps before publication. Unix
also checks file identity and status-change time. The recheck and rename are
separate operations. For an initially absent destination, `writeAtomically`
uses exclusive creation. A competing creator or a detected revision change reports
`fileAccess` and kind `DestinationChanged`, preserving the current file and
removing the temporary file.
Parent directories are never created. Success reports a
committed file, not guaranteed power-loss durability. A cancellation before
dispatch prevents work; a late read result is discarded, while a committed write
keeps its actual result. Hosts retain file dialogs, security-scoped access and
product import/export decisions. Errors expose an I/O kind without paths/content.

For user-confirmed replacement on local macOS APFS, capture the regular file
with Swift `PresetDocumentFiles.captureReplacement(at:)` before asking for
approval, and pass that observation to `replaceApproved(_:with:maximumByteCount:)`.
Rust exposes the same opt-in flow as `capture_replacement` and `replace_approved`.
.NET exposes it as `CaptureReplacementAsync` and `ReplaceApprovedAsync` on
`PresetDocumentFiles`.
The native adapter binds the observed parent and file revision to the path,
checks again before and after staging, and exchanges entries without creating
a missing destination. A detected change reports `destinationChanged`; other
platforms and volumes report `approvedReplacementUnsupported`. An application
can offer a new confirmation or save a separate copy.

An exchange is not an atomic comparison of revisions. An uncoordinated change
can happen after the last check. Every committed exchange leaves the displaced
entry at `displaced-preset` inside a private sibling recovery directory, even if
its file identity matches the approval. A matching identity does not mean the
file's bytes remained unchanged. An uncertain commit also retains recovery and
reports its last observed directory location. Reveal the recovery directory to
the user; do not automatically open, remove or restore the displaced item.
The retained entry is not an immutable copy, and these results do not promise
power-loss durability.

## Packages

### Swift

Swift consumers pin `https://github.com/naviapps/preset-documents.git` to a full
commit and select the `PresetDocuments` product. The tracked `CPresetDocuments`
XCFramework permits SwiftPM installation without a consumer-side Rust toolchain.
Authenticated repository access is configured by the consumer, never embedded here.

For a standalone executable, save this as `Package.swift`, replacing the revision
placeholder with a published full 40-character commit:

```swift
// swift-tools-version: 6.4
import PackageDescription

let package = Package(
  name: "MyPresets",
  platforms: [.macOS(.v26)],
  dependencies: [
    .package(
      url: "https://github.com/naviapps/preset-documents.git",
      revision: "<published-full-40-character-commit-revision>"),
  ],
  targets: [
    .executableTarget(
      name: "MyPresets",
      dependencies: [
        .product(name: "PresetDocuments", package: "preset-documents"),
      ]),
  ]
)
```

Save this codec example as `Sources/MyPresets/main.swift`, then run
`swift run MyPresets` from the consumer package directory:

```swift
import PresetDocuments

struct Layout: Codable { let columns: Int }
let format = try PresetFormat(identifier: "com.example.layout")
let document = format.makeDocument(payload: Layout(columns: 2))
let data = try PresetJSONCodec.encode(document)
let restored = try PresetJSONCodec.decodeDocument(
  Layout.self, from: data, matching: format,
  limits: .init(maximumByteCount: 1_000_000))
print(restored.payload.columns)
```

Add the `PresetDocumentsFileIO` product to the target only when using file APIs.
The [external Swift fixture](https://github.com/naviapps/preset-documents/blob/main/fixtures/swift-consumer/Sources/PresetDocumentsConsumer/Consumer.swift)
demonstrates both products in an owned temporary directory.

`PresetFormat` validates both explicit construction and `Codable` decoding through
Rust. Its three stored fields remain required when decoding; invalid identifiers or
version ranges report `PresetDocumentError.invalidConfiguration`.

### Rust

Rust consumers select the public `preset-documents` package from the private Git
repository. The independent `preset-documents-file-io` package is optional.
Use the same published full commit for both, replacing these placeholders:

```toml
[dependencies]
preset-documents = { git = "https://github.com/naviapps/preset-documents.git", rev = "FULL_40_CHARACTER_PUBLISHED_COMMIT_ID" }
preset-documents-file-io = { git = "https://github.com/naviapps/preset-documents.git", rev = "FULL_40_CHARACTER_PUBLISHED_COMMIT_ID" }
```

These packages are not published to crates.io. A normal client owns its
`Cargo.lock`: the SDK workspace lock does not freeze the client's registry
resolution. Git authentication belongs to the client's package delivery setup;
neither Rust package invokes SDK developer tooling or builds a native framework.

### C

`preset-documents-ffi` supplies the owned C ABI. A macOS C client uses the paired
`CPresetDocuments.framework`, including its shipped `Headers`, native library,
resources and notices. Copy it from the `macos-arm64_x86_64` slice under
`Artifacts/CPresetDocuments.xcframework` at the selected revision.
Include `<CPresetDocuments/preset_documents.h>` and link
with `-F /absolute/installed/framework/directory -framework CPresetDocuments`,
retaining a runtime search path to that installed directory. The client needs no
`crates/ffi/include` directory or Rust toolchain. Retain the original `ThirdParty`
notices with the payload; Windows DLL/import-library installation is separate.

See the [independent C client](https://github.com/naviapps/preset-documents/blob/main/fixtures/c-consumer/main.c) for codec and file calls
using only the installed header and framework.

### .NET

.NET consumers use `NaviApps.PresetDocuments`, `PresetFormat` and
`PresetJsonCodec`. NuGet installs the source-matched native library from the
appropriate macOS or Windows x64 package. No package feed
publication is part of local packing or CI.
See [package installation, examples and managed checks](https://github.com/naviapps/preset-documents/blob/main/bindings/dotnet/README.md).

### Shared native contract

Use Swift `PresetFormat.init(json:)` and `encodeJSON()` or .NET
`PresetFormat.FromJson` and `ToJson` to exchange format settings. Both use the
same Rust-validated UTF-8 object with `identifier`, `minimumSupportedVersion`
and `currentVersion`. C dispatcher operation 12 exposes this contract.

Swift's `normalizeDocumentEnvelope` and .NET's `NormalizeDocumentEnvelope` validate
a single document and normalize the envelope to UTF-8 JSON. Use the plural
methods for collections. Both call the same Rust codec as
`normalize_document_envelopes` and C dispatcher operation 2, which take an
explicit root. Payload JSON retains its number spelling, string escapes and
internal whitespace. The host decides how to interpret or migrate it.

The C result status distinguishes successful JSON results from typed error JSON.
Copy a borrowed byte view before freeing its owner. Inputs must be readable for their
lengths; null/size checks cannot establish arbitrary pointer validity. Do not free
or mutate buffers concurrently. Allocation failure is not a recoverable contract.
Swift result `Data` owns its bytes independently of later native calls or file changes.
Its storage releases a transferred native buffer when it no longer needs the bytes;
empty results release their native buffer immediately.
The existing dispatcher also provides operation 4: supply serialized limits in
`data`, with no options, to receive canonical validated limits JSON before encoding.
Operation 7 creates a new file without replacement and uses the same file
options as operations 5 and 6. It requires the native artifact from the same
revision as the bindings. Operation 11 compacts string escapes in completed
typed JSON using the same options as operation 3. Operation 14 takes UTF-8
timestamp text with no options and returns its canonical spelling, UTC whole
seconds, nanoseconds, original offset in seconds and a flag for discarded
nonzero subnanosecond digits. It requires the native artifact from the same
revision as the bindings.

## Development and acceptance

Developer commands use the immutable `sdk-tooling` revision in
`tools/tooling-sources.json`. See [shared developer setup](https://github.com/naviapps/sdk-tooling#readme)
for verified caches, offline use and source overrides. Ordinary SwiftPM and NuGet
consumers do not need that setup.

Use Swift 6.4, macOS 26+, Rust 1.99.1 and .NET 10. Select tools per command without
changing global configuration. Review `mise.toml` before trusting its scoped
configuration. The complete `make check` gate requires a clean committed checkout
because it includes independent installation from that exact Git revision.
On a new checkout or after a lockfile change,
run `cargo fetch --locked` with registry access before the offline Rust build
and tests. Rebuild the native framework after native input changes;
`tools/native.py --verify` checks source/output hashes and exports.

```sh
mise trust mise.toml
mise install
mise exec -- cargo fetch --locked
mise exec -- make check
```

The tracked framework is ready for checks after a fresh clone. After changing
native inputs, rebuild it with `mise exec -- make native`. Focused targets such
as `test`, `test-rust`, `test-bindings` and `docs` can validate working edits;
run the complete installation gate against the resulting clean committed snapshot.

`make check` verifies the existing framework and runs Rust, Swift, C ownership,
.NET fixtures, external Swift/Rust/C consumers, DocC and portable build-driver regressions. `make test-tools` runs only
the portable driver checks. The C and managed fixtures load the accepted native
library. A clean local NuGet consumer loads the packaged copy. Windows package
execution is a separate gate.
`mise exec -- make docs` checks both Swift API catalogs against this package's
symbol graphs without running file operations.

`make test-consumer` builds and runs the SDK-owned package in
`fixtures/swift-consumer/` against only installable inputs. It imports both public products,
round-trips a document through the native codec, and verifies create-only file
publication and collision rejection in an owned temporary directory. It uses
the tracked framework without rebuilding it. Shared
[consumer verification](https://github.com/naviapps/sdk-tooling/blob/main/docs/REFERENCE.md#consumer-verification)
defines staging, configuration/cache isolation, explicit Git/mirror policy and evidence limits.

`make test-installed-consumers` checks two other independent clients through
pinned shared SDK tooling. Both exercise UInt64/Unicode codec
round-trips, typed invalid-document rejection and create-only collision behavior
inside their own temporary directories. The Rust fixture uses this exact Git
revision and owns a new `Cargo.lock`; the C fixture sees only the copied public
framework, header and notices. The gate never rebuilds native artifacts.

The producer must be clean and committed. A shallow checkout needs a complete
verified `--git-source` repository or self-contained bundle; the driver does not
fetch missing private history. For example, from the SDK root on an owned macOS
worker with mise-managed tools:

```sh
mise exec -- python3 -B tools/install-consumers.py --revision FULL_40_CHARACTER_CURRENT_COMMIT_ID \
  --git-source /absolute/verified/preset-documents.git \
  --evidence /absolute/owned-artifacts/preset-installed-consumers.json
```

The external Rust client needs registry access for its initial fetch; the
producer's Cargo cache does not fill the client's fresh cache. Choose a new
`--evidence` path outside the checkout. `--language rust` or `--language c`
selects one boundary. This is local-mirror installation evidence and does not
test remote authentication or GitHub reachability. The macOS workflow requests
full history and runs this gate in its existing worker job.

GitHub Actions runs Rust and strict Swift tests on Xcode 27 and Rust and tool
tests on Windows x64. CI also exercises standalone Swift/Rust/C consumers,
managed package tests and the installed `win-x64` NuGet package. See [Windows package acceptance](bindings/dotnet/README.md#windows-package-acceptance)
for local x64 verification on Windows x64 or ARM64.

Original upstream notices and Rust runtime provenance remain under `ThirdParty/`.
The shared macOS builder combines the notice bodies verbatim in
`Resources/ThirdPartyNotices.txt`; NuGet packages retain the original notice files.

## Static checks

`mise install` prepares pinned developer tools. Run `mise run lint` before builds
or `mise run format` to format owned source. Compiler and native acceptance remain
in the existing check targets. Shared policy comes from the pinned SDK tooling;
vendored code, native artifacts and original notices are excluded.

## License

This project is available under the [MIT License](LICENSE). Third-party components
retain their original notices in [ThirdParty](ThirdParty).
