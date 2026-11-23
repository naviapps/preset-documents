# .NET document codec

## Installation and first use

The package targets .NET 10. Use the macOS variant on macOS 26 or later with
Apple Silicon or Intel .NET. On Windows 11 use the `win-x64` variant with x64
.NET, including when running an x64 process on Windows ARM64. The macOS and
Windows packages have the same ID and version. Use separate local feeds and
NuGet caches when restoring more than one variant.

On macOS, `mise exec -- make test-bindings` verifies the tracked framework,
builds the universal package in `.native-build/macos/packages` and verifies it
using a standalone .NET consumer. The packaged macOS dylib has its own ad hoc
signature. On Windows, use the [package acceptance command](#windows-package-acceptance)
to build the package and its `preset-documents-windows.json` receipt locally.
Verify the Windows package SHA-256 against the receipt.
In a new consumer directory, replace the example path with your verified local feed:

```sh
dotnet new console --framework net10.0
dotnet add package NaviApps.PresetDocuments --version 0.1.0 --source "<package-directory>"
```

Replace `Program.cs` with this example and run `dotnet run`:

```csharp
using NaviApps.PresetDocuments;

var format = new PresetFormat("com.example.presets");
var bytes = PresetJsonCodec.Encode(format.MakeDocument(42));
var restored = PresetJsonCodec.DecodeDocument<int>(bytes, format);
Console.WriteLine(restored.Payload);
```

Consumers need the package and its native asset, not Rust or the SDK's Python
developer tools. See the [repository guide](https://github.com/naviapps/preset-documents#readme)
for format, file semantics and maintainer prerequisites.

## Codec contract

Persist all three `PresetFormat` properties when saving its JSON representation.
A missing minimum or current version fails validation when loading it. C# callers
can use the convenience constructor's defaults when creating a new format.
For exchange with Swift, use `PresetFormat.ToJson()` and `PresetFormat.FromJson(data)`.
These methods use Rust's UTF-8 format JSON object with camelCase keys. The reader
rejects duplicate, missing and unknown fields before constructing the format.

`PresetJsonCodec` passes `PresetDocumentLimits` to the shared Rust codec for both
reads and writes. Depth includes the document envelope and, for collections, the
outer array. Managed JSON processing follows the requested depth; it does not
apply System.Text.Json's default depth of 64 or clamp a caller's configuration to
a second list of supported limits. Before write serialization, the native
validate-limits operation checks the configuration and returns validated limits
on success. Invalid limits propagate as `invalidConfiguration` before payload
converters or cyclic graphs are visited.
For a typed collection, the binding reads `IReadOnlyList.Count` after validating
limits. Rust checks that count before any payload converter runs.
`TooManyDocuments` reports the full declared count. Rust also checks the
collection that the serializer produces.
Reads likewise enter Rust before managed payload parsing.

On writes, the managed serializer allows one excess level so Rust can identify a
depth violation. If serialization stops on a deeper graph, Rust checks the
written prefix for a limit or configuration error. Otherwise the original host
serialization exception is preserved. Cyclic graphs remain bounded.

Rust independently checks original read bytes, the compact string spelling of
completed typed writes, and normalized output. Completed typed writes use the
same UTF-8 string escapes as Swift, including literal punctuation and
supplementary Unicode characters. The original transport to Rust is capped at
64 MiB and can use more bytes than the compact output. Failed managed writes
still report the byte count of the written prefix when that prefix exceeds its
input limit.
The binding checks UTF-16 before writing string, character and `Uri` values
or dictionary keys. An unpaired surrogate reports `InvalidDocument`. A literal
replacement character or valid surrogate pair retains its value. Host
converters own values they write directly, and Rust validates the resulting JSON.
Payload numbers remain opaque, and a `JsonElement` payload retains integer and
decimal tokens through the codec. Hosts still own their payload conversions.

`PresetMetadata.CreatedAtRfc3339` exposes the creation time as RFC3339 text,
including nanosecond fractions and dates or offsets outside .NET's range.
The existing `DateTimeOffset? CreatedAt` returns a value when .NET can represent
the timestamp exactly, including directly assigned text with trailing zeros
beyond .NET's seven fractional digits. Both properties are `null` when the
timestamp is absent.
Use `CreatedAtRfc3339` to read or set timestamps outside the typed range.
The Rust codec validates the text for projection and validates metadata again
on encode. Assigning `CreatedAt` replaces or clears the stored timestamp,
including any text read from an existing document.

Use `PresetJsonCodec.NormalizeDocumentEnvelope(data, format, limits)` for a
single document and `PresetJsonCodec.NormalizeDocumentEnvelopes(data, format, limits)`
for a collection. Both validate the selected root and return UTF-8 JSON with
normalized envelopes. Payload lexemes, including number spelling, string escapes
and internal whitespace, remain exact. Hosts own payload conversion and migration.

## Maintainer checks

Run these commands from the SDK root after the
[maintainer setup](https://github.com/naviapps/preset-documents#development-and-acceptance).
`mise exec -- make test-bindings` verifies the tracked macOS framework and runs
C ownership and managed tests, including the clean NuGet consumer. It uses the
tracked native library as the package asset.

To run only the managed depth, Unicode, number preservation and byte-budget cases,
restore the test project first, then select that source-matched native library:

```sh
mise exec -- dotnet restore bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj
PRESET_DOCUMENTS_NATIVE_LIBRARY=/absolute/path/to/native/library \
  mise exec -- dotnet run --no-restore \
  --project bindings/dotnet/tests/PresetDocuments.Tests/PresetDocuments.Tests.csproj \
  --configuration Release -- --depth-contracts
```

The ordinary managed acceptance entry point also runs these cases.

### Windows package acceptance

To build and run the x64 SDK on an owned Windows 11 ARM64 worker, use native
ARM64 Python and Rust, the Rust `x86_64-pc-windows-msvc` standard library, and
Visual Studio Build Tools with both ARM64 and x64 C++ components. Use the x64
variant of the .NET SDK version in `mise.toml` so the managed fixtures and NuGet
consumer can load the x64 DLL. The ARM64 and x64 .NET SDKs have separate
[installation paths](https://learn.microsoft.com/en-us/dotnet/core/install/windows#arm-based-windows-pcs).

```powershell
$env:DOTNET_BIN = 'C:\Program Files\dotnet\x64\dotnet.exe'
mise exec -- python -B tools/windows.py
```

The driver checks the x64 .NET executable and SDK version before building.
ARM64-hosted MSVC links the x64 DLL and C fixture. It checks the DLL exports,
the compiled C executable's machine and the native build's source receipt
before running the fixtures. Both managed checks use the x64 .NET host. The
clean package consumer restores `win-x64` from a temporary feed and checks the
exact packaged DLL. The receipt records the source commit, the hashes of the
build support files in this checkout, the compiler host and target, the .NET
SDK version, and whether the run used x64 emulation.

The x64 package and its hash receipt go to
`.native-build/win-x64/<version>` after the consumer passes. If `NAVI_CI_ARTIFACTS`
is set, the x64 package and receipt are also retained under `win-x64/<version>` in
that directory. Both copies are staged before either is replaced. If the external
replacement or verification fails, the driver restores the previous local output.
If backup cleanup fails after verification, the command reports that error with
both verified copies in place. Use a clean committed checkout or an exact VM
Runner workspace capture to tie the receipt to the tested source. This is
standalone SDK evidence.

GitHub Actions uses a Windows x64 runner to build the package and run the
installed package consumer. The local Windows command supports both x64 and
ARM64 hosts.

## File I/O

Use `PresetDocumentFiles.ReadAsync(path, maximumByteCount, cancellationToken)` for
bounded reads and `WriteAtomicallyAsync(data, path, maximumByteCount,
cancellationToken)` for atomic replacement. Paths must be absolute; no directory
is created. The binding rejects trailing directory separators, NUL characters and
path strings that cannot be encoded as UTF-8 without replacing characters.
A failure preserves the existing file. Once replacement has started,
a late cancellation does not turn a committed write into a cancellation error.
An already-cancelled write returns before copying the caller's bytes. Rust
checks the byte budget before the binding copies an accepted request. The
binding captures those bytes before returning the task.
`FileAccess` failures retain a native I/O kind in `Details` without file contents.
An atomic write uses exclusive creation for an initially absent destination.
A competing creator or a detected revision change reports `FileAccess` with
kind `DestinationChanged` and preserves the current file. Creating a file on a
volume without exclusive publication reports `AtomicNewUnsupported`.
Use `WriteNewAsync` with the same arguments to create a new file without
overwriting. An occupied name reports `DestinationAlreadyExists`; a platform
or filesystem without the required operation reports `AtomicNewUnsupported`.
The codec and file APIs share this assembly and native library. Their API boundary
does not remove native filesystem dependencies; codec calls perform no file I/O.

For a host confirmation to replace an existing revision, call
`CaptureReplacementAsync(path, cancellationToken)` before asking the user and
pass the returned observation to `ReplaceApprovedAsync(approval, data,
maximumByteCount, cancellationToken)` after confirmation. Both operations use the
same Rust contract as Swift and require local macOS APFS. On Windows or other
unsupported volumes, capture reports `ApprovedReplacementUnsupported`. A missing
or changed destination observed before exchange reports `DestinationChanged`.
The revision check and exchange are separate. Every committed exchange retains
the previous item at `DisplacedPresetPath`; even a matching identity does not
establish unchanged contents. Show `RecoveryDirectory` to the user and retain
it when publication cannot be verified. Keep the actual result of an operation
already dispatched when cancellation arrives.
