using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using NaviApps.PresetDocuments;

DepthContractTests.Run();
PresetFormatJsonTests.Run();
static void Require(bool value) { if (!value) throw new System.Diagnostics.UnreachableException("Preset contract failed."); }
var format = new PresetFormat("com.example.presets");
var opaquePayload = """["日本語😀", "\u0061\/"]""";
var envelope = "{\"payload\":" + opaquePayload + ",\"formatVersion\":{\"version\":1,\"identifier\":\"com.example.presets\"}}";
var envelopeInput = Encoding.UTF8.GetBytes(envelope);
var collectionInput = Encoding.UTF8.GetBytes("[" + envelope + "]");
var expectedEnvelope = "{\"formatVersion\":{\"identifier\":\"com.example.presets\",\"version\":1},\"metadata\":{},\"payload\":" + opaquePayload + "}";
var normalizedEnvelope = PresetJsonCodec.NormalizeDocumentEnvelope(envelopeInput, format);
var normalizedEnvelopes = PresetJsonCodec.NormalizeDocumentEnvelopes(collectionInput, format);
Require(Encoding.UTF8.GetString(normalizedEnvelope) == expectedEnvelope);
Require(Encoding.UTF8.GetString(normalizedEnvelopes) == "[" + expectedEnvelope + "]");
try { PresetJsonCodec.NormalizeDocumentEnvelope(collectionInput, format); throw new System.Diagnostics.UnreachableException("The single-document operation accepted an array."); }
catch (PresetDocumentException error) { Require(error.Code == PresetDocumentErrorCode.InvalidDocument); }
try { PresetJsonCodec.NormalizeDocumentEnvelopes(envelopeInput, format); throw new System.Diagnostics.UnreachableException("The collection operation accepted an object."); }
catch (PresetDocumentException error) { Require(error.Code == PresetDocumentErrorCode.InvalidDocument); }
try
{
    PresetJsonCodec.DecodeDocument<int>("""{"formatVersion":["com.example.presets",1],"payload":42}"""u8.ToArray(), format);
    throw new System.Diagnostics.UnreachableException("Positional document structure was accepted.");
}
catch (PresetDocumentException e) { Require(e.Code == PresetDocumentErrorCode.InvalidDocument); }
var arrayInput = Encoding.UTF8.GetBytes("""[{"formatVersion":{"identifier":"com.example.presets","version":1},"metadata":{"title":"日本語😀"},"payload":["日本語😀",9007199254740993,18446744073709551616]}]""");
var arrayDocuments = PresetJsonCodec.DecodeDocuments<JsonElement>(arrayInput, format, new(MaximumDocumentCount: 1));
Require(arrayDocuments.Count == 1 && arrayDocuments[0].Metadata.Title == "日本語😀");
Require(arrayDocuments[0].Payload[0].GetString() == "日本語😀");
Require(arrayDocuments[0].Payload[1].GetRawText() == "9007199254740993");
Require(arrayDocuments[0].Payload[2].GetRawText() == "18446744073709551616");
var arrayRestored = PresetJsonCodec.DecodeDocuments<JsonElement>(PresetJsonCodec.Encode(arrayDocuments), format);
Require(arrayRestored[0].Payload[2].GetRawText() == "18446744073709551616");
Console.WriteLine("Object-only document structures reject positional arrays and preserve Unicode/opaque array payloads.");
try
{
    PresetJsonCodec.DecodeDocument<JsonElement>(Encoding.UTF8.GetBytes("{\"formatVersion\":{\"identifier\":\"com.example.presets\",\"version\":1},\"payload\":\"\\ud800\"}"), format);
    throw new System.Diagnostics.UnreachableException("Invalid escaped Unicode was accepted.");
}
catch (PresetDocumentException e) { Require(e.Code == PresetDocumentErrorCode.InvalidDocument); }
foreach (string malformed in new[] { "\uD800", "\uDC00", "before\uD800x" })
{
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(format.MakeDocument(malformed)));
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(
        new[] { format.MakeDocument(42, new PresetMetadata(Title: malformed)) }));
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(
        format.MakeDocument(new Dictionary<string, string> { [malformed] = "value" })));
}
foreach (char malformed in new[] { '\uD800', '\uDC00' })
{
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(format.MakeDocument(malformed)));
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(
        format.MakeDocument(new Dictionary<char, string> { [malformed] = "value" })));
}
var replacementUri = new Uri("relative/\uFFFD", UriKind.Relative);
foreach (string malformed in new[] { "\uD800", "\uDC00" })
{
    var absolute = new Uri("https://example.test/" + malformed);
    var relative = new Uri("relative/" + malformed, UriKind.Relative);
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(format.MakeDocument(absolute)));
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(new[] { format.MakeDocument(new { Address = relative }) }));
    var keys = new Dictionary<Uri, int> { [relative] = 1, [replacementUri] = 2 };
    Require(keys.Count == 2);
    RequireInvalidManagedText(() => PresetJsonCodec.Encode(format.MakeDocument(keys)));
}
var escapedUri = new Uri("https://example.test/%E6%97%A5?view=%2f");
foreach (var address in new[] { escapedUri, replacementUri })
{
    var addressBytes = PresetJsonCodec.Encode(format.MakeDocument(address));
    Require(PresetJsonCodec.DecodeDocument<Uri>(addressBytes, format).Payload.OriginalString == address.OriginalString);
    var keyedAddress = format.MakeDocument(new Dictionary<Uri, int> { [address] = 1 });
    var restoredKeys = PresetJsonCodec.DecodeDocument<Dictionary<Uri, int>>(PresetJsonCodec.Encode(keyedAddress), format);
    Require(restoredKeys.Payload.Single().Key.OriginalString == address.OriginalString);
}
var hostAddress = format.MakeDocument(new HostUriPayload(new Uri("relative/\uD800", UriKind.Relative)));
var hostPayload = PresetJsonCodec.DecodeDocument<JsonElement>(PresetJsonCodec.Encode(hostAddress), format).Payload;
Require(hostPayload.GetProperty("address").GetString() == "https://example.test/host-converter");
const string intentionalUnicode = "日本語😀\uFFFD";
var unicodeDocument = format.MakeDocument(new Dictionary<string, string> { [intentionalUnicode] = intentionalUnicode },
    new PresetMetadata(Title: intentionalUnicode));
var restoredUnicode = PresetJsonCodec.DecodeDocument<Dictionary<string, string>>(PresetJsonCodec.Encode(unicodeDocument), format);
Require(restoredUnicode.Metadata.Title == intentionalUnicode && restoredUnicode.Payload[intentionalUnicode] == intentionalUnicode);
var characters = format.MakeDocument(new Dictionary<char, char> { ['雨'] = '\uFFFD' });
var restoredCharacters = PresetJsonCodec.DecodeDocument<Dictionary<char, char>>(PresetJsonCodec.Encode(characters), format);
Require(restoredCharacters.Payload['雨'] == '\uFFFD');

static void RequireInvalidManagedText(Action encode)
{
    try { encode(); }
    catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidDocument) { return; }
    throw new System.Diagnostics.UnreachableException("Ill-formed UTF-16 was silently serialized.");
}

var legacy = PresetJsonCodec.DecodeDocuments<Payload>(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "fixtures", "documents", "legacy-presetkit-2.json")), format);
Require(legacy.Count == 1 && legacy[0].Payload.Integer == 9_007_199_254_740_993 && legacy[0].Payload.Name == "日本語😀");
var original = format.MakeDocument(new Payload("日本語😀", 9_007_199_254_740_993), new PresetMetadata(Title: "Example", CreatedAt: DateTimeOffset.Parse("2026-10-30T12:34:56.1234567+09:00", System.Globalization.CultureInfo.InvariantCulture)));
var data = PresetJsonCodec.Encode(original);
foreach (var encoding in new[] { Encoding.UTF8, Encoding.BigEndianUnicode })
{
    var decoded = PresetJsonCodec.DecodeDocument<Payload>(encoding.GetBytes(Encoding.UTF8.GetString(data)), format);
    Require(decoded == original);
}
PresetMetadataTimestampTests.Run(format);
try { PresetJsonCodec.DecodeDocument<Payload>(data, format, new(MaximumByteCount: data.Length - 1)); throw new System.Diagnostics.UnreachableException("Byte limit ignored."); }
catch (PresetDocumentException e) { Require(e.Code == PresetDocumentErrorCode.InputTooLarge); }
try { PresetJsonCodec.DecodeDocument<Payload>(data, new PresetFormat("com.other.presets")); throw new System.Diagnostics.UnreachableException("Format mismatch ignored."); }
catch (PresetDocumentException e) { Require(e.Code == PresetDocumentErrorCode.FormatMismatch); }
var collection = PresetJsonCodec.Encode(new[] { original, original });
Require(PresetJsonCodec.DecodeDocuments<Payload>(collection, format).Count == 2);
try { PresetJsonCodec.DecodeDocuments<Payload>(collection, format, new(MaximumDocumentCount: 1)); throw new System.Diagnostics.UnreachableException("Document count ignored."); }
catch (PresetDocumentException e)
{
    Require(e.Code == PresetDocumentErrorCode.TooManyDocuments);
    Require(e.Details.GetProperty("documentCount").GetInt32() == 2);
    Require(e.Details.GetProperty("maximumDocumentCount").GetInt32() == 1);
}
Console.WriteLine("Managed native round-trip, Unicode, metadata, integer precision and shared error contracts passed.");
try { PresetJsonCodec.DecodeDocument<MisleadingPayload>(data, format); throw new System.Diagnostics.UnreachableException("Payload decoder escaped the boundary."); }
catch (PresetDocumentException e) { Require(e.Code == PresetDocumentErrorCode.InvalidDocument); }
var ioRoot = Path.Combine(Path.GetTempPath(), "preset-io-" + Guid.NewGuid());
Directory.CreateDirectory(ioRoot);
try
{
    var path = Path.Combine(ioRoot, "日本語.preset");
    await PresetDocumentFiles.WriteNewAsync(Encoding.UTF8.GetBytes("old"), path, 3);
    try { await PresetDocumentFiles.WriteNewAsync(Encoding.UTF8.GetBytes("competing"), path); throw new System.Diagnostics.UnreachableException("Create-only write replaced an existing file."); }
    catch (PresetDocumentException error) { Require(error.Code == PresetDocumentErrorCode.DestinationAlreadyExists); }
    Require(Encoding.UTF8.GetString(await PresetDocumentFiles.ReadAsync(path, 3)) == "old");
    try { await PresetDocumentFiles.ReadAsync(path, 2); throw new System.Diagnostics.UnreachableException("Read budget ignored."); }
    catch (PresetDocumentException error)
    {
        Require(error.Code == PresetDocumentErrorCode.InputTooLarge);
        Require(error.Details.GetProperty("byteCount").GetInt32() == 3);
        Require(error.Details.GetProperty("maximumByteCount").GetInt32() == 2);
    }
    await PresetDocumentFiles.WriteAtomicallyAsync(Encoding.UTF8.GetBytes("new"), path);
    using var cancelled = new CancellationTokenSource(); cancelled.Cancel();
    try { await PresetDocumentFiles.WriteAtomicallyAsync(Encoding.UTF8.GetBytes("cancelled"), path, cancellationToken: cancelled.Token); throw new System.Diagnostics.UnreachableException("Cancellation ignored."); }
    catch (OperationCanceledException) { }
    Require(File.ReadAllText(path) == "new");
}
finally { Directory.Delete(ioRoot, true); }
Console.WriteLine("File I/O round-trip, native error translation and pre-dispatch cancellation passed.");
await FileWriteAdmissionTests.RunAsync();
await ApprovedReplacementTests.RunAsync();

internal sealed record Payload(string Name, ulong Integer);
internal sealed record HostUriPayload([property: JsonConverter(typeof(HostUriConverter))] Uri Address);
internal sealed class HostUriConverter : JsonConverter<Uri>
{
    public override Uri Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options) =>
        throw new NotSupportedException();

    public override void Write(Utf8JsonWriter writer, Uri value, JsonSerializerOptions options) =>
        writer.WriteStringValue("https://example.test/host-converter");
}
[JsonConverter(typeof(MisleadingConverter))]
internal sealed record MisleadingPayload;
internal sealed class MisleadingConverter : JsonConverter<MisleadingPayload>
{
    public override MisleadingPayload Read(ref Utf8JsonReader reader, Type type, JsonSerializerOptions options)
    {
        _ = new PresetFormat("bad/id");
        throw new InvalidOperationException();
    }
    public override void Write(Utf8JsonWriter writer, MisleadingPayload value, JsonSerializerOptions options) => writer.WriteNullValue();
}
