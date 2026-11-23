using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using NaviApps.PresetDocuments;

internal static class PresetFormatJsonTests
{
    private static readonly JsonSerializerOptions Web = new(JsonSerializerDefaults.Web);
    private static readonly JsonSerializerOptions SnakeCase = new() { PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower };

    internal static void Run()
    {
        const string identifier = "com.example.presets";
        var defaults = new PresetFormat(identifier, currentVersion: 3);
        Require(defaults.MinimumSupportedVersion == 1 && defaults.CurrentVersion == 3);
        var authored = new PresetFormat(identifier: identifier, minimumSupportedVersion: 2, currentVersion: 3);
        Require(authored.MinimumSupportedVersion == 2 && authored.CurrentVersion == 3);
        RequireInvalidConfiguration(() => new PresetFormat(default!, default, default));

        var canonical = """{"identifier":"com.example.presets","minimumSupportedVersion":2,"currentVersion":3}"""u8.ToArray();
        var portable = PresetFormat.FromJson(canonical);
        Require(portable is { Identifier: identifier, MinimumSupportedVersion: 2, CurrentVersion: 3 });
        Require(portable.ToJson().SequenceEqual(canonical));
        var duplicate = """{"identifier":"com.example.presets","minimumSupportedVersion":2,"minimumSupportedVersion":1,"currentVersion":3}"""u8.ToArray();
        RequireInvalidConfiguration(() => PresetFormat.FromJson(duplicate));
        RequireInvalidConfiguration(() => PresetFormat.FromJson([0xff]));

        var readers = new[]
        {
            JsonSerializerOptions.Default,
            Web,
            SnakeCase,
            PresetFormatJsonContext.Default.Options
        };
        foreach (var options in readers)
        {
            string json = JsonSerializer.Serialize(authored, options);
            var saved = JsonNode.Parse(json)!.AsObject();
            string minimum = PropertyName(nameof(PresetFormat.MinimumSupportedVersion), options);
            string current = PropertyName(nameof(PresetFormat.CurrentVersion), options);
            Require(saved.Count == 3 && saved.ContainsKey(minimum) && saved.ContainsKey(current));
            var decoded = JsonSerializer.Deserialize<PresetFormat>(json, options);
            Require(decoded is { Identifier: identifier, MinimumSupportedVersion: 2, CurrentVersion: 3 });

            saved.Remove(minimum);
            RequireInvalidConfiguration(() => JsonSerializer.Deserialize<PresetFormat>(saved.ToJsonString(), options));
            saved[minimum] = 1;
            saved.Remove(current);
            RequireInvalidConfiguration(() => JsonSerializer.Deserialize<PresetFormat>(saved.ToJsonString(), options));
        }

        var caseInsensitive = JsonSerializer.Deserialize<PresetFormat>(
            """{"IDENTIFIER":"com.example.presets","MINIMUMSUPPORTEDVERSION":2,"CURRENTVERSION":3}""",
            Web);
        Require(caseInsensitive is { MinimumSupportedVersion: 2, CurrentVersion: 3 });
        Console.WriteLine("Stored preset formats require explicit versions with default, configured and source-generated JSON.");
    }

    private static string PropertyName(string name, JsonSerializerOptions options)
        => options.PropertyNamingPolicy?.ConvertName(name) ?? name;

    private static void RequireInvalidConfiguration(Func<PresetFormat?> read)
    {
        try { _ = read(); }
        catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.InvalidConfiguration) { return; }
        throw new UnreachableException("The preset format accepted an omitted or invalid version.");
    }

    private static void Require(bool value)
    {
        if (!value) throw new UnreachableException("Stored preset format field names or versions changed.");
    }
}

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase)]
[JsonSerializable(typeof(PresetFormat))]
internal sealed partial class PresetFormatJsonContext : JsonSerializerContext;
