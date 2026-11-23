using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using NaviApps.PresetDocuments;

internal static class DepthContractTests
{
    private const string Leaf = "[\"日本語😀\",9007199254740993,18446744073709551616,1.234567890123456789]";

    internal static void Run()
    {
        var format = new PresetFormat("com.example.presets");
        var failures = new List<string>();
        int checks = 0;
        void Check(string label, Action test)
        {
            ++checks;
            try { test(); }
            catch (Exception error) { failures.Add($"{label}: {error.GetType().Name}: {error.Message}"); }
        }

        foreach (bool collection in new[] { false, true })
        {
            string root = collection ? "collection" : "document";
            Check($"{root} managed codec preserves a payload at the native depth ceiling", () =>
            {
                var (input, payload) = Fixture(collection, 128);
                var document = Decode(input, format, collection, null);
                Require(document.Metadata.Title == "日本語😀");
                Require(document.Payload.GetRawText() == payload);
                Require(Decode(Encode(document, collection, null), format, collection, null).Payload.GetRawText() == payload);
            });
            Check($"{root} failed serializer preserves the written byte count", () => WrittenPrefix(format, collection));
            Check($"{root} typed UTF-8 text fits its exact byte budget", () => TypedTextBudget(format, collection));
            Check($"{root} typed string escapes fit their shared byte budget", () => TypedStringEscapeBudget(format, collection));
            Check($"{root} unset envelope metadata fits the compact byte budget", () => UnsetMetadataBudget(format, collection));
        }
        Check("custom depth reports the native maximum through managed reads and writes", () =>
        {
            const int maximumDepth = 4;
            var limits = new PresetDocumentLimits(MaximumDepth: maximumDepth);
            var (input, payload) = Fixture(collection: true, maximumDepth + 1);
            var error = ExpectError(() => Decode(input, format, true, limits), PresetDocumentErrorCode.NestingTooDeep);
            Require(error.Details.GetProperty("maximumDepth").GetInt32() == maximumDepth);

            using var tree = JsonDocument.Parse(payload);
            error = ExpectError(() => Encode(format.MakeDocument(tree.RootElement), true, limits), PresetDocumentErrorCode.NestingTooDeep);
            Require(error.Details.GetProperty("maximumDepth").GetInt32() == maximumDepth);
        });
        Check("invalid signed depth rejects before visiting a managed payload", () =>
        {
            var limits = new PresetDocumentLimits(MaximumDepth: -1);
            var (input, _) = Fixture(collection: false, depth: 4);
            ExpectError(() => Decode(input, format, false, limits), PresetDocumentErrorCode.InvalidConfiguration);

            var payload = new FailedWritePayload();
            ExpectError(() => Encode(format.MakeDocument(payload), false, limits), PresetDocumentErrorCode.InvalidConfiguration);
            Require(!payload.WasWritten);
        });
        Check("collection normalized output budget", () => CollectionOutputBudget(format));
        Check("typed collection checks its full count before visiting payloads", () =>
        {
            var payload = new FailedWritePayload();
            var document = format.MakeDocument(payload);
            var limits = new PresetDocumentLimits(MaximumDocumentCount: 1);
            var error = ExpectError(() => PresetJsonCodec.Encode(new[] { document, document, document }, limits),
                PresetDocumentErrorCode.TooManyDocuments);
            Require(error.Details.GetProperty("documentCount").GetInt32() == 3);
            Require(error.Details.GetProperty("maximumDocumentCount").GetInt32() == 1);
            Require(!payload.WasWritten);
        });
        Check("custom collection retains configuration precedence and native final count check", () =>
        {
            var document = format.MakeDocument(42);
            var documents = new[] { document, document, document };
            var limits = new PresetDocumentLimits(MaximumDocumentCount: 1);
            var countProbe = new CountedList<PresetDocument<int>>(documents, () => throw new InvalidOperationException());
            ExpectError(() => PresetJsonCodec.Encode(countProbe, limits with { MaximumDepth = 0 }),
                PresetDocumentErrorCode.InvalidConfiguration);

            var inaccurate = new CountedList<PresetDocument<int>>(documents, () => 1);
            var error = ExpectError(() => PresetJsonCodec.Encode(inaccurate, limits), PresetDocumentErrorCode.TooManyDocuments);
            Require(error.Details.GetProperty("documentCount").GetInt32() == 2);
        });
        Check("host serialization error is preserved", () =>
        {
            try { PresetJsonCodec.Encode(format.MakeDocument(new FailedWritePayload())); }
            catch (JsonException error) when (error.Message.Contains("Host write failed", StringComparison.Ordinal)) { return; }
            throw new System.Diagnostics.UnreachableException("The host serialization error was replaced.");
        });
        Check("typed payload depth and cycle remain bounded", () =>
        {
            object payload = "日本語😀";
            for (int i = 0; i < 127; ++i) payload = new object[] { payload };
            var decoded = PresetJsonCodec.DecodeDocument<JsonElement>(PresetJsonCodec.Encode(format.MakeDocument(payload)), format);
            var leaf = decoded.Payload;
            for (int i = 0; i < 127; ++i) leaf = leaf[0];
            Require(leaf.GetString() == "日本語😀");
            var error = ExpectError(() => PresetJsonCodec.Encode(format.MakeDocument(new object[] { payload })), PresetDocumentErrorCode.NestingTooDeep);
            Require(error.Details.GetProperty("maximumDepth").GetInt32() == 128);
            object[] cycle = new object[1];
            cycle[0] = cycle;
            error = ExpectError(() => PresetJsonCodec.Encode(format.MakeDocument(cycle), new(MaximumDepth: 8)), PresetDocumentErrorCode.NestingTooDeep);
            Require(error.Details.GetProperty("maximumDepth").GetInt32() == 8);
        });
        foreach (string failure in failures) Console.Error.WriteLine(failure);
        if (failures.Count != 0) throw new System.Diagnostics.UnreachableException($"Depth contracts: {failures.Count}/{checks} checks failed.");
        Console.WriteLine($"Depth contracts: {checks} checks passed against the selected native library.");
    }

    private static (byte[] Input, string Payload) Fixture(bool collection, int depth)
    {
        int wrappers = depth - (collection ? 2 : 1) - 1;
        string payload = new string('[', wrappers) + Leaf + new string(']', wrappers);
        string document = "{\"formatVersion\":{\"identifier\":\"com.example.presets\",\"version\":1},\"metadata\":{\"title\":\"日本語😀\"},\"payload\":" + payload + "}";
        return (Encoding.UTF8.GetBytes(collection ? "[" + document + "]" : document), payload);
    }

    private static PresetDocument<JsonElement> Decode(byte[] input, PresetFormat format, bool collection, PresetDocumentLimits? limits) =>
        collection ? PresetJsonCodec.DecodeDocuments<JsonElement>(input, format, limits).Single() : PresetJsonCodec.DecodeDocument<JsonElement>(input, format, limits);

    private static byte[] Encode<T>(PresetDocument<T> document, bool collection, PresetDocumentLimits? limits) =>
        collection ? PresetJsonCodec.Encode(new[] { document }, limits) : PresetJsonCodec.Encode(document, limits);

    private static void CollectionOutputBudget(PresetFormat format)
    {
        var (input, _) = Fixture(true, 65);
        input = Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(input).Replace(",\"metadata\":{\"title\":\"日本語😀\"}", ""));
        var error = ExpectError(() => Decode(input, format, true, new(MaximumByteCount: input.Length)), PresetDocumentErrorCode.OutputTooLarge);
        int outputBytes = error.Details.GetProperty("byteCount").GetInt32();
        Require(error.Details.GetProperty("maximumByteCount").GetInt32() == input.Length);
        Require(outputBytes > input.Length);
        CheckPayload(Decode(input, format, true, new(MaximumByteCount: outputBytes)).Payload);
    }

    private static void TypedTextBudget(PresetFormat format, bool collection)
    {
        const string url = "https://example.invalid/日本語?tag=雨";
        var metadata = new PresetMetadata(Id: "fixture", Title: "日本語", ProducerVersion: "1")
        {
            CreatedAtRfc3339 = "2026-10-30T12:34:56Z"
        };
        var document = format.MakeDocument(new TypedTextPayload(url), metadata);
        const string expectedDocument = """
            {"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{"id":"fixture","title":"日本語","createdAt":"2026-10-30T12:34:56Z","producerVersion":"1"},"payload":{"保存先":"https://example.invalid/日本語?tag=雨"}}
            """;
        byte[] expected = Encoding.UTF8.GetBytes(collection ? "[" + expectedDocument + "]" : expectedDocument);
        var bytes = Encode(document, collection, new(MaximumByteCount: expected.Length));
        Require(bytes.SequenceEqual(expected));
        Require(Decode(bytes, format, collection, null).Payload.GetProperty("保存先").GetString() == url);
        var error = ExpectError(() => Encode(document, collection, new(MaximumByteCount: expected.Length - 1)),
            PresetDocumentErrorCode.InputTooLarge);
        Require(error.Details.GetProperty("byteCount").GetInt32() == expected.Length);
        Require(error.Details.GetProperty("maximumByteCount").GetInt32() == expected.Length - 1);
    }

    private static void TypedStringEscapeBudget(PresetFormat format, bool collection)
    {
        const string text = "<雨>&'+`/😀\u00A0\u2028\uE000\"\\\n\u001F";
        const string property = "key/😀<";
        string quoted = "\"<雨>&'+`/😀\u00A0\u2028\uE000\\\"\\\\\\n\\u001f\"";
        string expectedDocument = "{\"formatVersion\":{\"identifier\":\"com.example.presets\",\"version\":1}," +
            "\"metadata\":{\"title\":" + quoted + "},\"payload\":{\"key/😀<\":" + quoted + "}}";
        byte[] expected = Encoding.UTF8.GetBytes(collection ? "[" + expectedDocument + "]" : expectedDocument);
        var document = format.MakeDocument(new Dictionary<string, string> { [property] = text }, new PresetMetadata(Title: text));
        var bytes = Encode(document, collection, new(MaximumByteCount: expected.Length));
        Require(bytes.SequenceEqual(expected));
        var restored = Decode(bytes, format, collection, null);
        Require(restored.Metadata.Title == text && restored.Payload.GetProperty(property).GetString() == text);
        var error = ExpectError(() => Encode(document, collection, new(MaximumByteCount: expected.Length - 1)),
            PresetDocumentErrorCode.InputTooLarge);
        Require(error.Details.GetProperty("byteCount").GetInt32() == expected.Length);
    }

    private static void UnsetMetadataBudget(PresetFormat format, bool collection)
    {
        const string expectedDocument = """
            {"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{},"payload":"日本語"}
            """;
        byte[] expected = Encoding.UTF8.GetBytes(collection ? "[" + expectedDocument + "]" : expectedDocument);
        var document = format.MakeDocument("日本語");
        Require(Encode(document, collection, new(MaximumByteCount: expected.Length)).SequenceEqual(expected));
        var error = ExpectError(() => Encode(document, collection, new(MaximumByteCount: expected.Length - 1)),
            PresetDocumentErrorCode.InputTooLarge);
        Require(error.Details.GetProperty("byteCount").GetInt32() == expected.Length);

        var metadataPayload = format.MakeDocument(new PresetMetadata());
        var payload = Decode(Encode(metadataPayload, collection, null), format, collection, null).Payload;
        Require(payload.GetProperty("title").ValueKind == JsonValueKind.Null);
        Require(payload.GetProperty("createdAt").ValueKind == JsonValueKind.Null);
    }

    private static void CheckPayload(JsonElement payload)
    {
        while (payload.GetArrayLength() == 1) payload = payload[0];
        Require(payload[0].GetString() == "日本語😀");
        Require(payload[1].GetRawText() == "9007199254740993");
        Require(payload[2].GetRawText() == "18446744073709551616");
        Require(payload[3].GetRawText() == "1.234567890123456789");
    }

    private static void WrittenPrefix(PresetFormat format, bool collection)
    {
        var payload = new WrittenPrefixFailurePayload();
        var error = ExpectError(
            () => Encode(format.MakeDocument(payload), collection, new(MaximumByteCount: 1)),
            PresetDocumentErrorCode.InputTooLarge);
        Require(payload.WrittenByteCount > 1);
        Require(error.Details.GetProperty("byteCount").GetInt64() == payload.WrittenByteCount);

        try { Encode(format.MakeDocument(new WrittenPrefixFailurePayload()), collection, null); }
        catch (JsonException failure) when (failure.Message.Contains("Host write prefix failed", StringComparison.Ordinal)) { return; }
        throw new System.Diagnostics.UnreachableException("The partial host serialization error was replaced.");
    }

    private static PresetDocumentException ExpectError(Action operation, PresetDocumentErrorCode code)
    {
        try { operation(); }
        catch (PresetDocumentException error) when (error.Code == code) { return error; }
        throw new System.Diagnostics.UnreachableException($"Expected {code}.");
    }

    private static void Require(bool value)
    {
        if (!value) throw new System.Diagnostics.UnreachableException("Depth contract failed.");
    }

    private sealed class CountedList<T>(IReadOnlyList<T> items, Func<int> count) : IReadOnlyList<T>
    {
        public int Count => count();
        public T this[int index] => items[index];
        public IEnumerator<T> GetEnumerator() => items.GetEnumerator();
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
}

internal sealed record TypedTextPayload([property: JsonPropertyName("保存先")] string Destination);

[JsonConverter(typeof(FailedWriteConverter))]
internal sealed record FailedWritePayload
{
    internal bool WasWritten { get; set; }
}

internal sealed class FailedWriteConverter : JsonConverter<FailedWritePayload>
{
    public override FailedWritePayload Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options) =>
        throw new NotSupportedException();
    public override void Write(Utf8JsonWriter writer, FailedWritePayload value, JsonSerializerOptions options)
    {
        value.WasWritten = true;
        throw new JsonException("Host write failed.");
    }
}

[JsonConverter(typeof(WrittenPrefixFailureConverter))]
internal sealed class WrittenPrefixFailurePayload
{
    internal long WrittenByteCount { get; set; }
}

internal sealed class WrittenPrefixFailureConverter : JsonConverter<WrittenPrefixFailurePayload>
{
    public override WrittenPrefixFailurePayload Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options) =>
        throw new NotSupportedException();
    public override void Write(Utf8JsonWriter writer, WrittenPrefixFailurePayload value, JsonSerializerOptions options)
    {
        writer.WriteStringValue("日本語😀");
        value.WrittenByteCount = writer.BytesCommitted + writer.BytesPending;
        throw new JsonException("Host write prefix failed.");
    }
}
