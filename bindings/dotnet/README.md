# PresetDocuments for .NET

Read and write versioned preset documents through the shared Rust codec. Your
application owns the payload schema and migrations.

## Installation and first use

The combined package targets .NET 10 and contains `osx-arm64`, `osx-x64`, and
`win-x64` native assets. It runs on macOS 26 or later and Windows 11, including
x64 .NET on Windows ARM64. NuGet chooses the asset for the .NET process or the
runtime identifier selected at publish time.

Download the package and its `.nupkg.json` receipt from a successful
[SDK distribution run](https://github.com/naviapps/preset-documents#distribution-checks).
Verify the package SHA-256 with the receipt. Pin the version of the downloaded
package and use a fresh cache to avoid reusing a platform-only build with the
same version. From a new consumer directory, run:

```sh
dotnet new console --framework net10.0 --no-restore
dotnet package add NaviApps.PresetDocuments --version "[0.1.0]" --no-restore
dotnet restore --source "<package-directory>" --packages ./.packages
```

Replace `Program.cs` with this example and run `dotnet run --no-restore`:

```csharp
using NaviApps.PresetDocuments;

var format = new PresetFormat("com.example.presets");
var bytes = PresetJsonCodec.Encode(format.MakeDocument(42));
var restored = PresetJsonCodec.DecodeDocument<int>(bytes, format);
Console.WriteLine(restored.Payload);
```

The native codec ships with the package. See the
[shared format and validation rules](https://github.com/naviapps/preset-documents#boundaries)
for the supported encodings, version range and limits.

## Codec contract

Use `PresetFormat.ToJson()` and `PresetFormat.FromJson(data)` to save or exchange
format settings with Swift. The UTF-8 object contains `identifier`,
`minimumSupportedVersion` and `currentVersion`. The reader requires all three
fields and rejects duplicates and unknown fields. The C# constructor offers
defaults when creating a new format.

`PresetJsonCodec` uses `PresetDocumentLimits` for reads and writes. Depth covers
the document envelope and the outer array for collections. Rust validates read
input before managed payload conversion and checks write limits and collection
counts before calling payload converters. Rust validation errors take precedence
when they apply to a failed serialization. Otherwise the original serializer
exception is preserved. A `JsonElement` payload retains integer and decimal
tokens. An unpaired UTF-16 surrogate in a string, character, `Uri` or dictionary
key reports `InvalidDocument`. Rust validates JSON written by custom converters.

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

## Native AOT and reflection-disabled apps

The generic `Encode<T>`, `DecodeDocument<T>` and `DecodeDocuments<T>` helpers
require System.Text.Json runtime reflection. For Native AOT or when
`JsonSerializerIsReflectionEnabledByDefault` is disabled, use the format,
metadata and file APIs with envelope normalization and supply your own payload
codec, such as source-generated System.Text.Json metadata.

`PresetJsonCodec.NormalizeDocumentEnvelope(data, format, limits)` validates a
single document. Use `NormalizeDocumentEnvelopes` for a collection. Both return
UTF-8 JSON with normalized envelopes and preserve the payload's number spelling,
string escapes and internal whitespace. On writes, use your payload codec to
construct the document JSON, then normalize it before saving.

Save this preset as `example.preset` in the consumer directory:

```json
{
  "formatVersion": { "identifier": "com.example.presets", "version": 1 },
  "payload": { "name": "Example" }
}
```

Replace `Program.cs` with the following and run
`dotnet run --no-restore -p:JsonSerializerIsReflectionEnabledByDefault=false`.
It reads the preset using the application's source-generated metadata and prints
`Example`.

```csharp
using System.Text.Json;
using System.Text.Json.Serialization;
using NaviApps.PresetDocuments;

var format = new PresetFormat("com.example.presets");
var limits = new PresetDocumentLimits();
var bytes = await PresetDocumentFiles.ReadAsync(
    Path.GetFullPath("example.preset"), limits.MaximumByteCount);
var normalized = PresetJsonCodec.NormalizeDocumentEnvelope(bytes, format, limits);
using var envelope = JsonDocument.Parse(normalized,
    new JsonDocumentOptions { MaxDepth = limits.MaximumDepth });
var payloadJson = new PresetPayloadJson(new JsonSerializerOptions
{
    PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
    MaxDepth = limits.MaximumDepth,
});
var payload = envelope.RootElement.GetProperty("payload")
    .Deserialize(payloadJson.PresetPayload)
    ?? throw new JsonException("Expected an object payload.");
Console.WriteLine(payload.Name);

internal sealed record PresetPayload(string Name);

[JsonSerializable(typeof(PresetPayload))]
internal sealed partial class PresetPayloadJson : JsonSerializerContext;
```

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

See the [shared file contract](https://github.com/naviapps/preset-documents#optional-file-io)
for symlinks, Finder tags, cancellation and recovery behavior.
