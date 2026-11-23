using System.Text.Json.Serialization;

namespace NaviApps.PresetDocuments;

internal sealed record FormatFields(
    [property: JsonRequired] string Identifier,
    [property: JsonRequired] long MinimumSupportedVersion,
    [property: JsonRequired] long CurrentVersion);

internal sealed record TimestampProjection(
    long UnixSeconds, int Nanoseconds, int OffsetSeconds, bool HasSubnanosecondPrecision);

internal readonly record struct EnvelopeOptions(FormatFields? Format, PresetDocumentLimits Limits, string Root);
internal readonly record struct CollectionCount(PresetDocumentLimits Limits, int DocumentCount);
internal readonly record struct FilePath(string Path);
internal readonly record struct FileOptions(string Path, int MaximumByteCount);
internal readonly record struct WriteBudget(int ByteCount, int MaximumByteCount);

[JsonSourceGenerationOptions(GenerationMode = JsonSourceGenerationMode.Metadata,
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase)]
[JsonSerializable(typeof(FormatFields))]
[JsonSerializable(typeof(TimestampProjection))]
[JsonSerializable(typeof(EnvelopeOptions))]
[JsonSerializable(typeof(CollectionCount))]
[JsonSerializable(typeof(FilePath))]
[JsonSerializable(typeof(FileOptions))]
[JsonSerializable(typeof(WriteBudget))]
internal sealed partial class PresetWireJson : JsonSerializerContext;
