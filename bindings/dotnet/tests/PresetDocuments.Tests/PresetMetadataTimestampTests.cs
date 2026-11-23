using System.Text.Json;
using NaviApps.PresetDocuments;

static class PresetMetadataTimestampTests
{
    internal static void Run(PresetFormat format)
    {
        VerifyPortableText(format);
        VerifyRejected(format);
        VerifyDateTimeOffsetProjection(format);
        Console.WriteLine("Managed RFC3339 codec routes and DateTimeOffset projection passed.");
    }

    private static void VerifyPortableText(PresetFormat format)
    {
        const string input = "2026-10-30 12:34:56.123456789+09:00";
        const string canonical = "2026-10-30T12:34:56.123456789+09:00";
        var authored = format.MakeDocument(42, new PresetMetadata { CreatedAtRfc3339 = input });
        Require(authored.Metadata.CreatedAtRfc3339 == input, input);

        var saved = JsonSerializer.SerializeToUtf8Bytes(WireDocument(input));
        var restored = PresetJsonCodec.DecodeDocument<int>(saved, format);
        Require(restored.Metadata.CreatedAtRfc3339 == canonical && restored.Payload == 42, input);
        var encoded = PresetJsonCodec.Encode(authored);
        using var output = JsonDocument.Parse(encoded);
        Require(Timestamp(output.RootElement) == canonical, input);
        Require(PresetJsonCodec.Encode(restored).SequenceEqual(encoded), input);

        var savedCollection = JsonSerializer.SerializeToUtf8Bytes(new[] { WireDocument(input) });
        var restoredCollection = PresetJsonCodec.DecodeDocuments<int>(savedCollection, format);
        Require(restoredCollection.Single() == restored, input);
        var encodedCollection = PresetJsonCodec.Encode(new[] { authored });
        using var outputCollection = JsonDocument.Parse(encodedCollection);
        Require(Timestamp(outputCollection.RootElement.EnumerateArray().Single()) == canonical, input);
        Require(PresetJsonCodec.Encode(restoredCollection).SequenceEqual(encodedCollection), input);
    }

    private static void VerifyRejected(PresetFormat format)
    {
        const string input = "2026-02-29T12:34:56Z";
        var metadata = new PresetMetadata { CreatedAtRfc3339 = input };
        Require(metadata.CreatedAt is null && metadata.CreatedAtRfc3339 == input, input);
        var document = format.MakeDocument(42, metadata);
        var saved = JsonSerializer.SerializeToUtf8Bytes(WireDocument(input));
        var collection = JsonSerializer.SerializeToUtf8Bytes(new[] { WireDocument(input) });
        Reject(() => PresetJsonCodec.Encode(document), input);
        Reject(() => PresetJsonCodec.Encode(new[] { document }), input);
        Reject(() => PresetJsonCodec.DecodeDocument<int>(saved, format), input);
        Reject(() => PresetJsonCodec.DecodeDocuments<int>(collection, format), input);
    }

    private static void VerifyDateTimeOffsetProjection(PresetFormat format)
    {
        var typedSecond = new DateTimeOffset(2026, 10, 30, 12, 34, 56, TimeSpan.Zero);
        var typedTimestamp = typedSecond.AddTicks(1_234_567);
        var typedMetadata = new PresetMetadata(CreatedAt: typedTimestamp);
        Require(typedMetadata == new PresetMetadata(CreatedAt: typedTimestamp.ToOffset(TimeSpan.FromHours(9))), "DateTimeOffset equality");
        Require(typedMetadata.CreatedAtRfc3339 == "2026-10-30T12:34:56.1234567Z", "DateTimeOffset formatting");
        var typedDocument = format.MakeDocument(42, typedMetadata);
        var typedRestored = PresetJsonCodec.DecodeDocument<int>(PresetJsonCodec.Encode(typedDocument), format);
        var (_, _, projectedTimestamp, _) = typedRestored.Metadata;
        Require(typedRestored == typedDocument && projectedTimestamp == typedTimestamp, "DateTimeOffset round trip");

        var exactCases = new[]
        {
            ("2026-10-30T12:34:56.123456700Z", typedTimestamp),
            ("2026-10-30T12:34:56.123456700+09:00", new DateTimeOffset(typedTimestamp.DateTime, TimeSpan.FromHours(9))),
            ("2026-10-30T12:34:56.000000000Z", typedSecond),
            ("2026-10-30t12:34:56.1234567z", typedTimestamp),
            ("2026-10-30 12:34:56.123456700Z", typedTimestamp),
            ("2026-10-30T12:34:56.123456700000000000Z", typedTimestamp),
            ("1969-12-31T23:29:59.875-00:30", DateTimeOffset.UnixEpoch.AddTicks(-1_250_000).ToOffset(TimeSpan.FromMinutes(-30))),
            ("0001-01-01T00:00:00Z", DateTimeOffset.MinValue),
        };
        foreach (var (input, expected) in exactCases)
        {
            var metadata = new PresetMetadata { CreatedAtRfc3339 = input };
            Require(metadata.CreatedAt?.EqualsExact(expected) == true, input);
            var bytes = PresetJsonCodec.Encode(format.MakeDocument(42, metadata));
            var restored = PresetJsonCodec.DecodeDocument<int>(bytes, format);
            Require(restored.Metadata.CreatedAt?.EqualsExact(expected) == true, input);
        }

        var inexactCases = new[]
        {
            "2026-10-30T12:34:56.12345678Z", "2026-10-30T12:34:56.123456789Z",
            "2026-10-30T12:34:56+15:00", "0000-01-01T00:00:00Z",
            "0001-01-01T00:00:00+00:01", "9999-12-31T23:59:59-00:01",
        };
        foreach (var input in inexactCases)
        {
            var metadata = new PresetMetadata { CreatedAtRfc3339 = input };
            Require(metadata.CreatedAt is null, input);
            var saved = JsonSerializer.SerializeToUtf8Bytes(WireDocument(input));
            var restored = PresetJsonCodec.DecodeDocument<int>(saved, format);
            Require(restored.Metadata.CreatedAt is null, input);
            Require((restored.Metadata with { CreatedAt = typedTimestamp }) == typedMetadata, input);
            Require((restored.Metadata with { CreatedAt = null }).CreatedAtRfc3339 is null, input);
        }
        const string subTickTimestamp = "2026-10-30T12:34:56.1234567001Z";
        Require(new PresetMetadata { CreatedAtRfc3339 = subTickTimestamp }.CreatedAt is null, subTickTimestamp);

        var missing = PresetJsonCodec.DecodeDocument<int>(PresetJsonCodec.Encode(format.MakeDocument(42)), format);
        Require(missing.Metadata.CreatedAt is null && missing.Metadata.CreatedAtRfc3339 is null, "omitted timestamp");
    }

    private static object WireDocument(string input) => new
    {
        formatVersion = new { identifier = "com.example.presets", version = 1 },
        metadata = new { createdAt = input },
        payload = 42,
    };

    private static string? Timestamp(JsonElement document) => document.GetProperty("metadata").GetProperty("createdAt").GetString();

    private static void Require(bool condition, string input)
    {
        if (!condition) throw new InvalidDataException($"Timestamp contract failed for [{input}].");
    }

    private static void Reject(Action operation, string input)
    {
        try { operation(); }
        catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidDocument) { return; }
        throw new InvalidDataException($"Invalid timestamp accepted [{input}].");
    }
}
