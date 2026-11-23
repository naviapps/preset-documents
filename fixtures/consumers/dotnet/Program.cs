using System.Text.Json;
using NaviApps.PresetDocuments;

var format = new PresetFormat("com.example.presets");
var savedFormat = JsonSerializer.Deserialize<PresetFormat>(
    """{"Identifier":"com.example.presets","MinimumSupportedVersion":2,"CurrentVersion":3}""");
if (savedFormat is not { MinimumSupportedVersion: 2, CurrentVersion: 3 })
    throw new InvalidDataException("The installed format reader changed the saved version range.");
var portableFormat = savedFormat.ToJson();
if (!portableFormat.AsSpan().SequenceEqual("""{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":3}"""u8)
    || PresetFormat.FromJson(portableFormat) is not { MinimumSupportedVersion: 2, CurrentVersion: 3 })
    throw new InvalidDataException("The installed codec changed the portable format JSON.");
try
{
    _ = JsonSerializer.Deserialize<PresetFormat>("""{"Identifier":"com.example.presets","CurrentVersion":3}""");
    throw new InvalidDataException("The installed format reader invented a missing version.");
}
catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidConfiguration) { }

const string authoredTimestamp = "1969-12-31t23:59:59.875z";
var authored = new PresetMetadata { CreatedAtRfc3339 = authoredTimestamp };
if (authored.CreatedAt != DateTimeOffset.UnixEpoch.AddTicks(-1_250_000)
    || authored.CreatedAtRfc3339 != authoredTimestamp)
    throw new InvalidDataException("The installed codec did not project the authored timestamp.");

const string timestamp = "2026-10-30T12:34:56.123456789+09:00";
var document = format.MakeDocument(ulong.MaxValue, new PresetMetadata { CreatedAtRfc3339 = timestamp });
try
{
    _ = PresetJsonCodec.Encode(new[] { document, document, document }, new(MaximumDocumentCount: 1));
    throw new InvalidDataException("The installed codec accepted too many authored documents.");
}
catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.TooManyDocuments)
{
    if (error.Details.GetProperty("documentCount").GetInt32() != 3
        || error.Details.GetProperty("maximumDocumentCount").GetInt32() != 1)
        throw new InvalidDataException("The installed codec did not report the full typed collection count.");
}
var data = PresetJsonCodec.Encode(document);
var restored = PresetJsonCodec.DecodeDocument<ulong>(data, format);
if (restored.Payload != ulong.MaxValue || restored.Metadata.CreatedAtRfc3339 != timestamp)
    throw new InvalidDataException("The installed codec changed the payload or timestamp.");

var minimal = format.MakeDocument("<日本語>&😀");
var minimalBytes = PresetJsonCodec.Encode(minimal);
if (!System.Text.Encoding.UTF8.GetString(minimalBytes).Contains("\"payload\":\"<日本語>&😀\""))
    throw new InvalidDataException("The installed codec expanded compact string escapes.");
if (!PresetJsonCodec.Encode(minimal, new(MaximumByteCount: minimalBytes.Length)).SequenceEqual(minimalBytes))
    throw new InvalidDataException("The installed codec did not honor the exact compact byte budget.");

try
{
    _ = PresetJsonCodec.Encode(format.MakeDocument(42, new PresetMetadata(Title: "\uD800")));
    throw new InvalidDataException("The installed codec replaced malformed UTF-16 in a metadata field.");
}
catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidDocument) { }

var malformedUri = new Uri("relative/\uD800", UriKind.Relative);
var validUri = new Uri("relative/\uFFFD", UriKind.Relative);
try
{
    _ = PresetJsonCodec.Encode(format.MakeDocument(new Dictionary<Uri, int> { [malformedUri] = 1, [validUri] = 2 }));
    throw new InvalidDataException("The installed codec rewrote a malformed URI key to a valid key.");
}
catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidDocument) { }
const string uriText = "https://example.test/%E6%97%A5?view=%2f";
var uriBytes = PresetJsonCodec.Encode(format.MakeDocument(new Uri(uriText)));
if (PresetJsonCodec.DecodeDocument<Uri>(uriBytes, format).Payload.OriginalString != uriText)
    throw new InvalidDataException("The installed codec changed a valid URI's spelling.");

var directory = Directory.CreateTempSubdirectory("preset-consumer-");
try
{
    var destination = Path.Combine(directory.FullName, "日本語.preset");
    try
    {
        await PresetDocumentFiles.WriteNewAsync(data, destination, maximumByteCount: data.Length - 1);
        throw new InvalidDataException("The installed file adapter allowed an oversized write.");
    }
    catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.OutputTooLarge) { }
    if (File.Exists(destination)) throw new InvalidDataException("A refused write created a file.");

    await PresetDocumentFiles.WriteNewAsync(data, destination);
    try
    {
        await PresetDocumentFiles.WriteNewAsync([], destination);
        throw new InvalidDataException("Create-only write replaced an existing file.");
    }
    catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.DestinationAlreadyExists) { }
    var saved = await PresetDocumentFiles.ReadAsync(destination);
    if (!saved.SequenceEqual(data))
        throw new InvalidDataException("The installed file adapter changed the bytes.");
    if (!PresetJsonCodec.NormalizeDocumentEnvelope(saved, format).SequenceEqual(saved))
        throw new InvalidDataException("The installed codec changed an already normalized document.");

    var atomicDestination = Path.Combine(directory.FullName, "atomic.preset");
    await PresetDocumentFiles.WriteAtomicallyAsync(data, atomicDestination);
    if (!(await PresetDocumentFiles.ReadAsync(atomicDestination)).SequenceEqual(data)
        || directory.GetFileSystemInfos().Length != 2)
        throw new InvalidDataException("Atomic creation changed the encoded bytes or left staged files.");

    PresetReplacementRevision approval;
    try
    {
        approval = await PresetDocumentFiles.CaptureReplacementAsync(destination);
    }
    catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.ApprovedReplacementUnsupported)
    {
        Console.WriteLine("Clean NuGet consumer passed. Approved replacement reported unsupported.");
        return;
    }
    var replacement = PresetJsonCodec.Encode(format.MakeDocument(42UL));
    var outcome = await PresetDocumentFiles.ReplaceApprovedAsync(approval, replacement);
    if (outcome is not PresetReplacementOutcome.Committed
        || !(await PresetDocumentFiles.ReadAsync(outcome.DisplacedPresetPath)).SequenceEqual(data)
        || !(await PresetDocumentFiles.ReadAsync(destination)).SequenceEqual(replacement))
        throw new InvalidDataException("The installed replacement adapter lost the retained or committed bytes.");
    Console.WriteLine("Clean NuGet consumer passed, including approved replacement recovery.");
}
finally { directory.Delete(recursive: true); }
