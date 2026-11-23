using System.Text;
using System.Text.Json;
using NaviApps.PresetDocuments;

AppContext.SetSwitch("System.Text.Json.JsonSerializer.IsReflectionEnabledByDefault", false);
if (JsonSerializer.IsReflectionEnabledByDefault)
    throw new InvalidOperationException("The consumer must disable JSON reflection.");

var format = new PresetFormat("com.example.presets", minimumSupportedVersion: 1, currentVersion: 2);
var roundTrip = PresetFormat.FromJson(format.ToJson());
if (roundTrip.Identifier != format.Identifier || roundTrip.MinimumSupportedVersion != 1 || roundTrip.CurrentVersion != 2)
    throw new InvalidDataException("The portable format lost its identifier or version range.");

const string timestamp = "1969-12-31t23:59:59.875z";
var metadata = new PresetMetadata { CreatedAtRfc3339 = timestamp };
if (metadata.CreatedAt != DateTimeOffset.UnixEpoch.AddTicks(-1_250_000) || metadata.CreatedAtRfc3339 != timestamp)
    throw new InvalidDataException("The native timestamp projection changed the instant or its original spelling.");

const string payload = """{ "integer":9007199254740993, "decimal":1.2500, "name":"\u65e5" }""";
var document = Encoding.UTF8.GetBytes($$"""{"formatVersion":{"identifier":"com.example.presets","version":1},"payload":{{payload}}}""");
var normalized = PresetJsonCodec.NormalizeDocumentEnvelope(document, format);
var collection = PresetJsonCodec.NormalizeDocumentEnvelopes([(byte)'[', .. document, (byte)']'], format);
if (!Encoding.UTF8.GetString(normalized).Contains(payload, StringComparison.Ordinal)
    || !collection.AsSpan().SequenceEqual([(byte)'[', .. normalized, (byte)']']))
    throw new InvalidDataException("Envelope normalization changed opaque payload JSON.");

var directory = Directory.CreateTempSubdirectory("preset-envelope-consumer-");
try
{
    var path = Path.Combine(directory.FullName, "日本語.preset");
    await PresetDocumentFiles.WriteNewAsync(normalized, path);
    try
    {
        await PresetDocumentFiles.WriteAtomicallyAsync(document, path, maximumByteCount: document.Length - 1);
        throw new InvalidDataException("The file adapter accepted an oversized write.");
    }
    catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.OutputTooLarge) { }
    if (!(await PresetDocumentFiles.ReadAsync(path)).SequenceEqual(normalized))
        throw new InvalidDataException("The file adapter changed the stored bytes.");

    try
    {
        var approval = await PresetDocumentFiles.CaptureReplacementAsync(path);
        var replacement = """{"formatVersion":{"identifier":"com.example.presets","version":2},"payload":42}"""u8.ToArray();
        var result = await PresetDocumentFiles.ReplaceApprovedAsync(approval, replacement);
        if (result is not PresetReplacementOutcome.Committed
            || !(await PresetDocumentFiles.ReadAsync(result.DisplacedPresetPath)).SequenceEqual(normalized)
            || !(await PresetDocumentFiles.ReadAsync(path)).SequenceEqual(replacement))
            throw new InvalidDataException("Approved replacement lost committed or retained bytes.");
        Console.WriteLine("Approved replacement retained the original preset.");
    }
    catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.ApprovedReplacementUnsupported)
    {
        Console.WriteLine("Approved replacement reported unsupported.");
    }
}
finally { directory.Delete(recursive: true); }

Console.WriteLine("Portable format, opaque envelopes, timestamp projection and files passed with JSON reflection disabled.");
