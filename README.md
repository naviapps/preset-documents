# PresetDocuments

Rust-backed versioned preset documents for Swift, Rust, C and .NET.

Swift sources and tests live in `Sources/` and `Tests/`; Rust crates live in
`crates/`, with C ABI tests in `crates/ffi/tests/c/`. .NET projects use
`bindings/dotnet/src/<project>/` and `bindings/dotnet/tests/<project>/`.
`fixtures/documents/` contains shared sample documents, `fixtures/consumers/`
contains independent SDK clients, and `distribution/` contains SDK-specific
package checks. The native frameworks live in `Artifacts/`.

## Start here

- [Swift, Rust and C installation](#packages) and [the .NET guide](bindings/dotnet/README.md).
- Swift API examples: [codec](#swift) and [file I/O](#optional-file-io).
- [Maintainer setup and acceptance](#development-and-acceptance).

Swift products require Swift 6.3 and macOS 26 or later. The tracked framework
contains ARM64 and Intel slices. Rust uses the pinned 1.99.1 toolchain. The .NET 10
NuGet package contains native assets for macOS ARM64, macOS x64, and Windows x64.
See [Windows package acceptance](#windows-package-acceptance)
for maintainer checks, including x64 emulation on Windows ARM64.

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

```swift
import Foundation
import PresetDocumentsFileIO

func exportReplacingApprovedFile(
  _ data: Data, at destination: URL,
  confirm: (URL) async -> Bool
) async throws -> PresetReplacementOutcome? {
  let revision = try await PresetDocumentFiles.captureReplacement(at: destination)
  guard await confirm(destination) else { return nil }
  return try await PresetDocumentFiles.replaceApproved(revision, with: data)
}
```

## Packages

### Swift

Swift consumers pin `https://github.com/naviapps/preset-documents.git` to a full
commit and select the `PresetDocuments` product. The tracked `CPresetDocuments`
XCFramework permits SwiftPM installation without a consumer-side Rust toolchain.
Authenticated repository access is configured by the consumer, never embedded here.

For a standalone executable, save this as `Package.swift`, replacing the revision
placeholder with a published full 40-character commit:

```swift
// swift-tools-version: 6.3
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
The [external Swift fixture](https://github.com/naviapps/preset-documents/blob/main/fixtures/consumers/swift/Sources/PresetDocumentsConsumer/Consumer.swift)
demonstrates both products in an owned temporary directory.

`PresetFormat` validates both explicit construction and `Codable` decoding through
Rust. Its three stored fields remain required when decoding; invalid identifiers or
version ranges report `PresetDocumentError.invalidConfiguration`.

### Rust

Rust consumers install `preset-documents` from the public Git repository.
Add `preset-documents-file-io` for file APIs. Pin both packages to the same commit:

```toml
[dependencies]
preset-documents = { git = "https://github.com/naviapps/preset-documents.git", rev = "FULL_40_CHARACTER_PUBLISHED_COMMIT_ID" }
preset-documents-file-io = { git = "https://github.com/naviapps/preset-documents.git", rev = "FULL_40_CHARACTER_PUBLISHED_COMMIT_ID" }
```

The client owns its `Cargo.lock` and resolves its registry dependencies.

### C

`preset-documents-ffi` supplies the owned C ABI. A macOS C client uses the paired
`CPresetDocuments.framework`, including its shipped `Headers`, native library,
resources and notices. Copy it from the `macos-arm64_x86_64` slice under
`Artifacts/CPresetDocuments.xcframework` at the selected revision.
Include `<CPresetDocuments/preset_documents.h>` and link
with `-F /absolute/installed/framework/directory -framework CPresetDocuments`,
retaining a runtime search path to that installed directory. The framework
contains the native SDK and C declarations. Retain the original `ThirdParty`
notices with the payload.

See the [independent C client](https://github.com/naviapps/preset-documents/blob/main/fixtures/consumers/c/main.c) for codec and file calls
using only the installed header and framework.

### .NET

.NET consumers use `NaviApps.PresetDocuments`, `PresetFormat` and
`PresetJsonCodec`. NuGet installs the appropriate source-matched native asset
from the combined macOS and Windows x64 package. The distribution workflow
retains the verified package as a CI artifact.
See [package installation and .NET examples](bindings/dotnet/README.md).

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

The C result status distinguishes successful bytes from typed error JSON.
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

C clients using ABI 2 pass the exact bytes from capture operation 8 to
`preset_documents_replace_approved` alongside the replacement data and write limit.
The captured revision identifies the destination. Keep it in memory for its
approval flow. The legacy operation 9 continues to accept its original JSON
arguments. The Swift and .NET bindings carry the revision as opaque bytes.

## Development and acceptance

Developer commands run from this checkout using the tools specified in
`mise.toml` and `rust-toolchain.toml`.

Use Swift 6.3 or newer, macOS 26+, the pinned Rust toolchain and .NET 10. Select tools per command without
changing global configuration. Review `mise.toml` before trusting its scoped
configuration.
On a new checkout or after a lockfile change,
run `cargo fetch --locked` with registry access before the offline Rust build
and tests.

```sh
mise trust mise.toml
mise install
mise exec -- cargo fetch --locked
mise exec -- make check
```

The tracked framework is ready for checks after a fresh clone. `make check` runs
source lint, Rust Clippy and tests, the complete .NET suite, strict Swift tests
and the standalone SwiftPM consumer. The lint and SwiftPM consumer commands use
the public SDK Tooling CLI at the immutable revision in `mise.toml`. Mise and uv
prepare it without a separate checkout or GitHub credentials.

Run `mise exec -- make lint` or `mise exec -- make format` for source checks and formatting.
These targets exclude vendored code, native artifacts and original notices.

`make test-consumer` builds and runs the SDK-owned package in
`fixtures/consumers/swift/` against only installable inputs. It imports both public products,
round-trips a document through the native codec, and checks the write budget,
create-only collision rejection, and approved replacement recovery on APFS in an
owned temporary directory. It uses the tracked framework without rebuilding it
and isolates the SwiftPM configuration and caches from the developer checkout.

GitHub Actions runs the Swift tests and standalone SwiftPM consumer on macOS 26
with Xcode 26.6 and on macOS 27 with Xcode 27.2. Public Windows x64 CI runs Rust
tests and exercises the .NET binding against a source-built DLL.

### Fuzz the Rust codec

The optional `documents` target exercises single and collection envelope
normalization on arbitrary bytes. Its seeds cover saved UTF-16 and UTF-32,
deeply nested JSON and exact numeric payloads. Successful normalization must
preserve payload lexemes, stay within the output budget and be idempotent.
Use the separate fuzz workspace on macOS or Linux for a bounded local run.

```sh
rustup toolchain install nightly-2026-11-23 --profile minimal
cargo install cargo-fuzz --locked --version 0.13.2 --root fuzz/target/tools
cargo +nightly-2026-11-23 fetch --locked --manifest-path fuzz/Cargo.toml
mkdir -p fuzz/corpus/documents
PATH="$PWD/fuzz/target/tools/bin:$PATH" cargo +nightly-2026-11-23 fuzz run documents \
  fuzz/corpus/documents fuzz/seeds/documents -- -max_len=4096 -max_total_time=30
```

The fuzzer writes new inputs to the ignored `fuzz/corpus/documents/` directory.
Add a focused stable Rust test when an input exposes a regression.

### Distribution checks

Run `mise exec -- make check-distribution` from a clean, committed SDK checkout.
Mise and uv select the public SDK Tooling revision pinned in `mise.toml`.
This checks the C and .NET bindings, the isolated NuGet consumer, the installed
Rust and C clients, and the portable build-driver tests. The
shared tools verify the tracked framework's source and output hashes and C exports.
The `[native]` section of [`sdk-tooling.toml`](sdk-tooling.toml) declares the
framework build and its receipt inputs. Rebuild the framework with
`mise exec -- make native` after changing native inputs.
The native build generates `crates/ffi/include/preset_documents.h` and
`crates/ffi/module.modulemap` from the Rust FFI. Git ignores these local outputs.
Run `mise exec -- make generate-header` to generate them separately.
`make check-header` compares the portable outputs with the C header and module
map shipped in the XCFramework.

Run `mise exec -- make test-managed` for the .NET suite against the tracked macOS
framework. `mise exec -- make test-bindings` also checks C ownership and tests the
typed codec and reflection-disabled APIs in clean consumers of the macOS development
package. That package and its receipt are written to `.native-build/packages`.

The public SDK Tooling repository runs the commands under `[distribution]` in
[`sdk-tooling.toml`](sdk-tooling.toml) on macOS and Windows x64. Dispatch the
workflow with the full SDK commit SHA on `main`:

```sh
gh workflow run check-sdk-distribution.yml -R naviapps/sdk-tooling \
  -f sdk=preset-documents -f "revision=$(git rev-parse HEAD)"
```

The run builds and tests the Windows x64 DLL, then combines it with the rebuilt
macOS native assets in one NuGet package. The macOS and Windows consumers
restore the same package bytes. Download the `.nupkg` and `.nupkg.json` from the
run's `preset-documents-nuget-…` artifact. Pass `-f windows-on-arm=true` to the
dispatch command to also verify the x64 package on GitHub's Windows on Arm runner.

`make test-installed-consumers` checks two other independent clients.
Both exercise UInt64/Unicode codec
round-trips, typed invalid-document rejection and create-only collision behavior
inside their own temporary directories. The Rust fixture uses this exact Git
revision and owns a new `Cargo.lock`; the C fixture sees only the copied public
framework, header and notices. The gate never rebuilds native artifacts.

The producer must be clean and committed. A shallow checkout needs a complete
verified `--git-source` repository or self-contained bundle; the driver does not
fetch missing history. For example, from the SDK root on an owned macOS
worker with mise-managed tools:

```sh
mise run sdk-tooling -- rust-c-consumers --revision FULL_40_CHARACTER_CURRENT_COMMIT_ID \
  --git-source /absolute/verified/preset-documents.git \
  --evidence /absolute/owned-artifacts/preset-installed-consumers.json
```

The external Rust client needs registry access for its initial fetch; the
producer's Cargo cache does not fill the client's fresh cache. Choose a new
`--evidence` path outside the checkout. `--language rust` or `--language c`
selects one boundary. This is local-mirror installation evidence and does not
test remote authentication or GitHub reachability. The maintainer's macOS
distribution job requests full history to run this gate.

Original upstream notices and Rust runtime provenance remain under `ThirdParty/`.
The shared macOS builder combines the notice bodies verbatim in
`Resources/ThirdPartyNotices.txt`; NuGet packages retain the original notice files.

#### Windows package acceptance

Use an owned Windows 11 runner with Python, Rust, the
`x86_64-pc-windows-msvc` target and Visual Studio Build Tools. On ARM64, use
ARM64 Python and Rust and install both ARM64 and x64 C++ components. On either
host, use the x64 .NET SDK version pinned in [`global.json`](global.json).
On Windows ARM64, its default path is `C:\Program Files\dotnet\x64\dotnet.exe`.
Run `mise install` in the SDK checkout to prepare cbindgen and the other pinned
developer tools.

```powershell
$env:DOTNET_BIN = 'C:\Program Files\dotnet\x64\dotnet.exe' # Set the x64 SDK path on this host.
mise run sdk-tooling -- python distribution/verify_windows.py
```

The driver generates the C header, uses it to verify x64 native exports and run
the C tests, then runs the .NET tests and restores the x64 development package
into a fresh consumer. Its receipt records source,
build inputs, compiler host and target, .NET SDK version and use of emulation.
Use a clean committed checkout or an exact VM Runner workspace capture to tie
the receipt to the tested source.

The accepted package and receipt are written to `.native-build/win-x64/<version>`.
Set `VM_RUNNER_ARTIFACTS_DIR` to retain them under `win-x64/<version>` there as well.
The driver stages both copies before publishing and restores the previous local
output if external publication or verification fails.

## License

This project is available under the [MIT License](LICENSE). Third-party components
retain their original notices in [ThirdParty](ThirdParty).
