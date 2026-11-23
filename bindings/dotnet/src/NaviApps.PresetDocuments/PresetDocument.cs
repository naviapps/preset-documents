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

/// <summary>The reason a codec or file operation was refused or failed.</summary>
public enum PresetDocumentErrorCode
{
    /// <summary>The format, operation options, or limits are invalid.</summary>
    InvalidConfiguration,
    /// <summary>The document's envelope, Unicode, or metadata is invalid.</summary>
    InvalidDocument,
    /// <summary>The stored format identifier differs from the requested format.</summary>
    FormatMismatch,
    /// <summary>The stored version is outside the supported range.</summary>
    UnsupportedFormatVersion,
    /// <summary>The received bytes exceed the input budget.</summary>
    InputTooLarge,
    /// <summary>The produced or submitted bytes exceed the output budget.</summary>
    OutputTooLarge,
    /// <summary>The collection exceeds the configured document count.</summary>
    TooManyDocuments,
    /// <summary>The JSON nesting exceeds the configured depth.</summary>
    NestingTooDeep,
    /// <summary>The file adapter could not perform the requested access.</summary>
    FileAccess,
    /// <summary>The native operation did not return a recognized result.</summary>
    NativeFailure,
    /// <summary>The create-only destination is already occupied.</summary>
    DestinationAlreadyExists,
    /// <summary>The destination cannot atomically publish a new entry without replacement.</summary>
    AtomicNewUnsupported,
    /// <summary>The observed destination changed after capture.</summary>
    DestinationChanged,
    /// <summary>The destination does not support approved replacement.</summary>
    ApprovedReplacementUnsupported
}

/// <summary>A typed failure reported by the native preset contract.</summary>
public sealed class PresetDocumentException : Exception
{
    /// <summary>The stable error category.</summary>
    public PresetDocumentErrorCode Code { get; }
    /// <summary>The structured fields supplied for this native error.</summary>
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

/// <summary>The document's format identity and version.</summary>
/// <param name="Identifier">The format identifier.</param>
/// <param name="Version">The version written by the exporting host.</param>
public sealed record PresetFormatVersion(string Identifier, long Version);

/// <summary>Optional metadata carried alongside the host's payload.</summary>
/// <param name="Id">An optional host-supplied identifier without storage semantics.</param>
/// <param name="Title">The display title supplied by the host.</param>
/// <param name="CreatedAt">A creation timestamp within the range representable by .NET.</param>
/// <param name="ProducerVersion">The exporting host's version, when supplied.</param>
public sealed record PresetMetadata(string? Id = null, string? Title = null, DateTimeOffset? CreatedAt = null, string? ProducerVersion = null)
{
    private const string TimestampFormat = "yyyy-MM-dd'T'HH:mm:ss.FFFFFFF";
    private const string UtcFormat = TimestampFormat + "'Z'";
    private const string OffsetFormat = TimestampFormat + "zzz";
    private readonly DateTimeOffset? _createdAt = CreatedAt;
    private readonly string? _createdAtText;

    /// <summary>Creates empty metadata.</summary>
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
/// <summary>Bounds for reading and writing document envelopes.</summary>
/// <param name="MaximumByteCount">The separate maximum for input and output bytes.</param>
/// <param name="MaximumDocumentCount">The maximum number of documents in a collection.</param>
/// <param name="MaximumDepth">The maximum JSON depth, including the envelope and collection root.</param>
public sealed record PresetDocumentLimits(int MaximumByteCount = 16 * 1024 * 1024, int MaximumDocumentCount = 65_536, int MaximumDepth = 128);

/// <summary>A versioned envelope carrying a host-owned payload.</summary>
/// <typeparam name="T">The payload type chosen by the host.</typeparam>
/// <param name="FormatVersion">The stored format identity and version.</param>
/// <param name="Metadata">Optional document metadata, represented by an empty instance when absent.</param>
/// <param name="Payload">The value managed by the host's payload codec.</param>
public sealed record PresetDocument<T>(PresetFormatVersion FormatVersion, PresetMetadata Metadata, T Payload);

/// <summary>The format a host writes and the inclusive range of versions it reads.</summary>
public sealed class PresetFormat
{
    private sealed record Fields(
        [property: JsonRequired] string Identifier,
        [property: JsonRequired] long MinimumSupportedVersion,
        [property: JsonRequired] long CurrentVersion);

    /// <summary>The validated dot-separated format identifier.</summary>
    public string Identifier { get; }
    /// <summary>The oldest accepted stored version.</summary>
    public long MinimumSupportedVersion { get; }
    /// <summary>The newest accepted version and the version used for new documents.</summary>
    public long CurrentVersion { get; }
    /// <summary>Validates the identifier and version range before creating the format.</summary>
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

    /// <summary>Validates the format fields supplied by the JSON serializer.</summary>
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

    /// <summary>Wraps a payload in a document using the current format version.</summary>
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

    /// <summary>Encodes a single document to UTF-8 JSON within the supplied limits.</summary>
    public static byte[] Encode<T>(PresetDocument<T> document, PresetDocumentLimits? limits = null) =>
        EncodeValue(EncodedDocument<T>.From(document), "document", limits);
    /// <summary>Encodes a document collection to a UTF-8 JSON array.</summary>
    public static byte[] Encode<T>(IReadOnlyList<PresetDocument<T>> documents, PresetDocumentLimits? limits = null) =>
        EncodeValue(documents?.Select(EncodedDocument<T>.From), "collection", limits,
            documents is null ? null : () => documents.Count);
    /// <summary>Validates and decodes one document against the expected format.</summary>
    public static PresetDocument<T> DecodeDocument<T>(byte[] data, PresetFormat format, PresetDocumentLimits? limits = null) => DecodeValue<PresetDocument<T>>(data, format, "document", limits);
    /// <summary>Validates and decodes a document collection against the expected format.</summary>
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
            try { _ = Native.Call(3, output.WrittenSpan, options); }
            catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidDocument) { }
            // An incomplete prefix alone is not a document error. Preserve the
            // host serializer failure unless Rust identifies a limit/configuration error.
            throw;
        }
        writer.Flush();
        return Native.Call(11, output.WrittenSpan, options);
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
