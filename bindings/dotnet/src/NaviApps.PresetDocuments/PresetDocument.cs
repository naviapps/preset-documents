using System.Buffers;
using System.ComponentModel;
using System.Globalization;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.Json.Serialization.Metadata;
using System.Text.Unicode;

namespace NaviApps.PresetDocuments;

public enum PresetDocumentErrorCode
{
    InvalidConfiguration, InvalidDocument, FormatMismatch, UnsupportedFormatVersion,
    InputTooLarge, OutputTooLarge, TooManyDocuments, NestingTooDeep, FileAccess, NativeFailure,
    DestinationAlreadyExists, AtomicNewUnsupported, DestinationChanged, ApprovedReplacementUnsupported
}

public sealed class PresetDocumentException : Exception
{
    public PresetDocumentErrorCode Code { get; }
    public JsonElement Details { get; }
    internal PresetDocumentException(JsonElement error) : base(error.ToString())
    {
        Details = error.Clone();
        Code = error.GetProperty("code").GetString() switch
        {
            "fileAccess" => PresetDocumentErrorCode.FileAccess,
            "destinationAlreadyExists" => PresetDocumentErrorCode.DestinationAlreadyExists,
            "atomicNewUnsupported" => PresetDocumentErrorCode.AtomicNewUnsupported,
            "destinationChanged" => PresetDocumentErrorCode.DestinationChanged,
            "approvedReplacementUnsupported" => PresetDocumentErrorCode.ApprovedReplacementUnsupported,
            "invalidConfiguration" => PresetDocumentErrorCode.InvalidConfiguration,
            "invalidDocument" => PresetDocumentErrorCode.InvalidDocument,
            "formatMismatch" => PresetDocumentErrorCode.FormatMismatch,
            "unsupportedFormatVersion" => PresetDocumentErrorCode.UnsupportedFormatVersion,
            "inputTooLarge" => PresetDocumentErrorCode.InputTooLarge,
            "outputTooLarge" => PresetDocumentErrorCode.OutputTooLarge,
            "tooManyDocuments" => PresetDocumentErrorCode.TooManyDocuments,
            "nestingTooDeep" => PresetDocumentErrorCode.NestingTooDeep,
            _ => PresetDocumentErrorCode.NativeFailure
        };
    }
}

public sealed record PresetFormatVersion(string Identifier, long Version);
public sealed record PresetMetadata(string? Id = null, string? Title = null, DateTimeOffset? CreatedAt = null, string? ProducerVersion = null)
{
    private const string TimestampFormat = "yyyy-MM-dd'T'HH:mm:ss.FFFFFFF";
    private const string UtcFormat = TimestampFormat + "'Z'";
    private const string OffsetFormat = TimestampFormat + "zzz";
    private readonly DateTimeOffset? _createdAt = CreatedAt;
    private readonly string? _createdAtText;

    [JsonConstructor]
    public PresetMetadata() : this(Id: null) { }

    /// <summary>The creation time when .NET can represent it exactly.</summary>
    [JsonIgnore]
    public DateTimeOffset? CreatedAt
    {
        get => _createdAt;
        init
        {
            _createdAt = value;
            _createdAtText = null;
        }
    }

    /// <summary>The RFC3339 creation time. The native codec validates it on write.</summary>
    [JsonPropertyName("createdAt")]
    public string? CreatedAtRfc3339
    {
        get => _createdAtText ?? FormatTimestamp(_createdAt);
        init
        {
            _createdAt = ProjectTimestamp(value);
            // Keep text that cannot be reconstructed from the typed value without loss.
            _createdAtText = value == FormatTimestamp(_createdAt) ? null : value;
        }
    }

    private sealed record TimestampProjection(
        long UnixSeconds, int Nanoseconds, int OffsetSeconds, bool HasSubnanosecondPrecision);

    private static DateTimeOffset? ProjectTimestamp(string? value)
    {
        if (value is null) return null;
        TimestampProjection projection;
        try
        {
            projection = JsonSerializer.Deserialize<TimestampProjection>(
                Native.Call(14, Encoding.UTF8.GetBytes(value)), Native.Json)
                ?? throw new InvalidOperationException("Missing native timestamp projection.");
        }
        catch (PresetDocumentException error) when (error.Code is PresetDocumentErrorCode.InvalidDocument
            or PresetDocumentErrorCode.InputTooLarge)
        {
            return null;
        }
        const int nanosecondsPerTick = 100;
        if (projection.HasSubnanosecondPrecision || projection.Nanoseconds % nanosecondsPerTick != 0)
            return null;

        try
        {
            return DateTimeOffset.FromUnixTimeSeconds(projection.UnixSeconds)
                .AddTicks(projection.Nanoseconds / nanosecondsPerTick)
                .ToOffset(TimeSpan.FromSeconds(projection.OffsetSeconds));
        }
        catch (ArgumentException)
        {
            // DateTimeOffset limits both its UTC/local calendar years and its offset.
            return null;
        }
    }

    private static string? FormatTimestamp(DateTimeOffset? value) => value is { } date
        ? date.ToString(date.Offset == TimeSpan.Zero ? UtcFormat : OffsetFormat, CultureInfo.InvariantCulture)
        : null;
}
public sealed record PresetDocumentLimits(int MaximumByteCount = 16 * 1024 * 1024, int MaximumDocumentCount = 65_536, int MaximumDepth = 128);
public sealed record PresetDocument<T>(PresetFormatVersion FormatVersion, PresetMetadata Metadata, T Payload);

public sealed class PresetFormat
{
    private sealed record Fields(
        [property: JsonRequired] string Identifier,
        [property: JsonRequired] long MinimumSupportedVersion,
        [property: JsonRequired] long CurrentVersion);

    public string Identifier { get; }
    public long MinimumSupportedVersion { get; }
    public long CurrentVersion { get; }
    public PresetFormat(string identifier, long minimumSupportedVersion = 1, long currentVersion = 1)
        : this(Validate(new Fields(identifier, minimumSupportedVersion, currentVersion))) { }

    private PresetFormat(Fields fields)
    {
        Identifier = fields.Identifier;
        MinimumSupportedVersion = fields.MinimumSupportedVersion;
        CurrentVersion = fields.CurrentVersion;
    }

    private static Fields Validate(Fields fields)
    {
        _ = Native.Call(1, JsonSerializer.SerializeToUtf8Bytes(fields, Native.Json));
        return fields;
    }

    // Missing JSON versions reach Rust as zero. Distinct parameter names preserve existing C# named calls.
    [JsonConstructor]
    [EditorBrowsable(EditorBrowsableState.Never)]
    [OverloadResolutionPriority(-1)]
    public PresetFormat(long MinimumSupportedVersion, long CurrentVersion, string Identifier)
        : this(Identifier, MinimumSupportedVersion, CurrentVersion) { }

    /// <summary>Reads the portable UTF-8 format JSON object and validates its complete version range.</summary>
    public static PresetFormat FromJson(byte[] data)
    {
        ArgumentNullException.ThrowIfNull(data);
        var canonical = Native.Call(12, data);
        var fields = JsonSerializer.Deserialize<Fields>(canonical, Native.Json)
            ?? throw new InvalidOperationException("Missing native format result.");
        return new(fields);
    }

    /// <summary>Writes the portable UTF-8 format JSON object for exchange with other bindings.</summary>
    public byte[] ToJson() => Native.Call(12, JsonSerializer.SerializeToUtf8Bytes(this, Native.Json));

    public PresetDocument<T> MakeDocument<T>(T payload, PresetMetadata? metadata = null) =>
        new(new(Identifier, CurrentVersion), metadata ?? new(), payload);
}

/// <summary>Portable JSON document processing. Payload schemas, migrations and file selection belong to hosts.</summary>
/// <remarks>Envelopes, formatVersion and non-null metadata must be JSON objects;
/// positional arrays are rejected, while payload arrays remain valid.
/// Collection parsing stops before decoding the first excess element. TooManyDocuments
/// reports the limit plus one, not the full array length. Whole-input byte, Unicode/string and
/// depth checks precede collection parsing. Managed JSON processing follows the requested
/// depth; Rust validates limits before payload serialization or parsing and enforces
/// document depth and byte budgets on reads and writes.</remarks>
public static class PresetJsonCodec
{
    private static readonly JavaScriptEncoder DocumentTextEncoder = JavaScriptEncoder.Create(UnicodeRanges.All);
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private static readonly StrictDocumentStringConverter DocumentStrings = new();
    private static readonly StrictDocumentCharacterConverter DocumentCharacters = new();
    private static readonly StrictDocumentUriConverter DocumentUris = new();

    private sealed record EncodedMetadata(
        [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Id,
        [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? Title,
        [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? CreatedAt,
        [property: JsonIgnore(Condition = JsonIgnoreCondition.WhenWritingNull)] string? ProducerVersion);

    private sealed record EncodedDocument<T>(PresetFormatVersion FormatVersion, EncodedMetadata? Metadata, T Payload)
    {
        internal static EncodedDocument<T>? From(PresetDocument<T>? document)
        {
            if (document is null) return null;
            var metadata = document.Metadata is { } value
                ? new EncodedMetadata(value.Id, value.Title, value.CreatedAtRfc3339, value.ProducerVersion)
                : null;
            return new(document.FormatVersion, metadata, document.Payload);
        }
    }

    /// <summary>Validates a single document and normalizes its envelope to UTF-8 JSON.</summary>
    /// <remarks>The payload retains its exact number spelling, escapes and internal whitespace.</remarks>
    public static byte[] NormalizeDocumentEnvelope(byte[] data, PresetFormat format, PresetDocumentLimits? limits = null)
        => NormalizeEnvelopes(data, format, "document", limits);

    /// <summary>Validates a document collection and normalizes only its envelopes to UTF-8 JSON.</summary>
    /// <remarks>Payload JSON retains exact lexemes, including numbers, escapes and internal
    /// whitespace. Hosts own payload interpretation and migration; this is not whole-document canonicalization.</remarks>
    public static byte[] NormalizeDocumentEnvelopes(byte[] data, PresetFormat format, PresetDocumentLimits? limits = null)
        => NormalizeEnvelopes(data, format, "collection", limits);

    public static byte[] Encode<T>(PresetDocument<T> document, PresetDocumentLimits? limits = null) =>
        EncodeValue(EncodedDocument<T>.From(document), "document", limits);
    public static byte[] Encode<T>(IReadOnlyList<PresetDocument<T>> documents, PresetDocumentLimits? limits = null) =>
        EncodeValue(documents?.Select(EncodedDocument<T>.From), "collection", limits,
            documents is null ? null : () => documents.Count);
    public static PresetDocument<T> DecodeDocument<T>(byte[] data, PresetFormat format, PresetDocumentLimits? limits = null) => DecodeValue<PresetDocument<T>>(data, format, "document", limits);
    public static IReadOnlyList<PresetDocument<T>> DecodeDocuments<T>(byte[] data, PresetFormat format, PresetDocumentLimits? limits = null) => DecodeValue<PresetDocument<T>[]>(data, format, "collection", limits);
    private static JsonSerializerOptions DocumentJson(int maximumDepth) => new(Native.Json) { MaxDepth = maximumDepth };
    private static byte[] EncodeValue<T>(T value, string root, PresetDocumentLimits? limits, Func<int>? documentCount = null)
    {
        limits ??= new();
        var options = new { format = (PresetFormat?)null, limits, root };
        _ = Native.Call(4, JsonSerializer.SerializeToUtf8Bytes(limits, Native.Json));
        if (documentCount is not null)
            _ = Native.Call(13, JsonSerializer.SerializeToUtf8Bytes(new { limits, documentCount = documentCount() }, Native.Json));
        // Allow the first excess level to reach Rust, including when a deeper
        // graph makes managed serialization stop. This is not a native limit.
        int transportDepth = (int)Math.Clamp((long)limits.MaximumDepth + 1, 1, int.MaxValue);
        var json = DocumentJson(transportDepth);
        json.Encoder = DocumentTextEncoder;
        json.Converters.Add(DocumentStrings);
        json.Converters.Add(DocumentCharacters);
        json.Converters.Add(DocumentUris);
        var output = new ArrayBufferWriter<byte>();
        using var writer = new Utf8JsonWriter(output, new JsonWriterOptions { MaxDepth = transportDepth, Encoder = DocumentTextEncoder });
        try { JsonSerializer.Serialize(writer, value, json); }
        catch (JsonException)
        {
            writer.Flush();
            // The incomplete prefix retains its original transport byte count.
            try { _ = Native.CallWritten(3, output, options); }
            catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidDocument) { }
            // An incomplete prefix alone is not a document error. Preserve the
            // host serializer failure unless Rust identifies a limit/configuration error.
            throw;
        }
        writer.Flush();
        return Native.CallWritten(11, output, options);
    }
    private static T DecodeValue<T>(byte[] input, PresetFormat format, string root, PresetDocumentLimits? limits)
    {
        limits ??= new();
        var bytes = NormalizeEnvelopes(input, format, root, limits);
        try { return JsonSerializer.Deserialize<T>(bytes, DocumentJson(limits.MaximumDepth)) ?? throw new JsonException(); }
        catch (Exception failure) when (failure is not OutOfMemoryException and not AccessViolationException)
        {
            throw InvalidDocument();
        }
    }

    private static byte[] NormalizeEnvelopes(byte[] data, PresetFormat format, string root, PresetDocumentLimits? limits)
        => Native.Call(2, data, new { format, limits = limits ?? new(), root });

    private static PresetDocumentException InvalidDocument()
    {
        using var error = JsonDocument.Parse("{\"code\":\"invalidDocument\"}");
        return new(error.RootElement);
    }

    private static void RequireUnicode(string value)
    {
        // System.Text.Json would silently replace an unpaired surrogate.
        try { _ = StrictUtf8.GetByteCount(value); }
        catch (EncoderFallbackException) { throw InvalidDocument(); }
    }

    private sealed class StrictDocumentStringConverter : JsonConverter<string>
    {
        public override string? Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
            => reader.GetString();

        public override string ReadAsPropertyName(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
            => reader.GetString()!;

        public override void Write(Utf8JsonWriter writer, string value, JsonSerializerOptions options)
        {
            RequireUnicode(value);
            writer.WriteStringValue(value);
        }

        public override void WriteAsPropertyName(Utf8JsonWriter writer, string value, JsonSerializerOptions options)
        {
            RequireUnicode(value);
            writer.WritePropertyName(value);
        }
    }

    private sealed class StrictDocumentCharacterConverter : JsonConverter<char>
    {
        public override char Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
            => reader.GetString() is [char value] ? value : throw new JsonException("Expected a single character.");

        public override char ReadAsPropertyName(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
            => Read(ref reader, type, options);

        public override void Write(Utf8JsonWriter writer, char value, JsonSerializerOptions options)
        {
            if (char.IsSurrogate(value)) throw InvalidDocument();
            ReadOnlySpan<char> text = stackalloc char[] { value };
            writer.WriteStringValue(text);
        }

        public override void WriteAsPropertyName(Utf8JsonWriter writer, char value, JsonSerializerOptions options)
        {
            if (char.IsSurrogate(value)) throw InvalidDocument();
            ReadOnlySpan<char> text = stackalloc char[] { value };
            writer.WritePropertyName(text);
        }
    }

    private sealed class StrictDocumentUriConverter : JsonConverter<Uri>
    {
        public override Uri? Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
            => JsonMetadataServices.UriConverter.Read(ref reader, type, options);

        public override Uri ReadAsPropertyName(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
            => JsonMetadataServices.UriConverter.ReadAsPropertyName(ref reader, type, options)!;

        public override void Write(Utf8JsonWriter writer, Uri value, JsonSerializerOptions options)
        {
            RequireUnicode(value.OriginalString);
            JsonMetadataServices.UriConverter.Write(writer, value, options);
        }

        public override void WriteAsPropertyName(Utf8JsonWriter writer, Uri value, JsonSerializerOptions options)
        {
            RequireUnicode(value.OriginalString);
            JsonMetadataServices.UriConverter.WriteAsPropertyName(writer, value, options);
        }
    }
}
