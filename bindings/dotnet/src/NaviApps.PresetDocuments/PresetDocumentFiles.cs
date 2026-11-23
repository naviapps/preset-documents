using System.Text;
using System.Text.Json;

namespace NaviApps.PresetDocuments;

/// <summary>An opaque observation of a destination to hold during one approval flow.</summary>
public sealed class PresetReplacementRevision
{
    private readonly byte[] token;
    internal ReadOnlySpan<byte> Token => token;

    internal PresetReplacementRevision(byte[] token) => this.token = token;
}

/// <summary>The observed identity of the displaced item. Its contents may have changed.</summary>
public enum PresetDisplacedIdentity
{
    /// <summary>The observed identity matches the approved file.</summary>
    MatchesApprovedFile,
    /// <summary>The displaced item has a different identity.</summary>
    Different,
    /// <summary>The displaced identity could not be verified.</summary>
    Unverified
}

/// <summary>The result of approved replacement, with the retained recovery directory.</summary>
public abstract record PresetReplacementOutcome(string RecoveryDirectory)
{
    /// <summary>The recovery entry for the displaced preset. It may be absent after an uncertain commit.</summary>
    public string DisplacedPresetPath => Path.Combine(RecoveryDirectory, "displaced-preset");

    /// <summary>The exchange committed and retained the displaced item.</summary>
    public sealed record Committed(string RecoveryDirectory, PresetDisplacedIdentity DisplacedIdentity)
        : PresetReplacementOutcome(RecoveryDirectory);

    /// <summary>Publication could not be verified. Retain the recovery directory for inspection.</summary>
    public sealed record CommitUncertain(string RecoveryDirectory) : PresetReplacementOutcome(RecoveryDirectory);
}

/// <summary>Optional file adapter; the host owns paths and import/export policy.</summary>
public static class PresetDocumentFiles
{
    private static readonly UTF8Encoding StrictUtf8 = new(encoderShouldEmitUTF8Identifier: false, throwOnInvalidBytes: true);

    private static string Absolute(string path)
    {
        if (!Path.IsPathFullyQualified(path) || Path.EndsInDirectorySeparator(path) || path.Contains('\0'))
            throw new ArgumentException("An absolute file path is required.", nameof(path));
        try
        {
            // JSON would replace unpaired UTF-16 surrogates before Rust saw the path.
            _ = StrictUtf8.GetByteCount(path);
        }
        catch (EncoderFallbackException)
        {
            throw new ArgumentException("The file path must be representable as UTF-8.", nameof(path));
        }
        return path;
    }

    /// <summary>Reads an opened regular file within the byte budget.</summary>
    public static async Task<byte[]> ReadAsync(string path, int maximumByteCount = 64 * 1024 * 1024, CancellationToken cancellationToken = default)
    {
        var options = new FileOptions(Absolute(path), maximumByteCount);
        return await ObserveAsync(() => Native.Call(5, [], options, PresetWireJson.Default.FileOptions), cancellationToken).ConfigureAwait(false);
    }

    /// <summary>Publishes a complete file at the destination, replacing an existing regular file.</summary>
    /// <remarks>An initially absent destination is created exclusively. A dispatched write reports its actual result after cancellation.</remarks>
    public static Task WriteAtomicallyAsync(byte[] data, string path, int maximumByteCount = 64 * 1024 * 1024, CancellationToken cancellationToken = default)
        => WriteAsync(6, data, path, maximumByteCount, cancellationToken);

    /// <summary>Atomically publishes a new file without replacing any existing destination.</summary>
    public static Task WriteNewAsync(byte[] data, string path, int maximumByteCount = 64 * 1024 * 1024, CancellationToken cancellationToken = default)
        => WriteAsync(7, data, path, maximumByteCount, cancellationToken);

    /// <summary>Captures an existing regular file on macOS APFS before the host asks for approval.</summary>
    /// <remarks>Other platforms and volumes report <see cref="PresetDocumentErrorCode.ApprovedReplacementUnsupported"/>.</remarks>
    public static Task<PresetReplacementRevision> CaptureReplacementAsync(string path, CancellationToken cancellationToken = default)
    {
        var options = new FilePath(Absolute(path));
        return ObserveAsync(() => new PresetReplacementRevision(Native.Call(8, [], options, PresetWireJson.Default.FilePath)), cancellationToken);
    }

    /// <summary>Rechecks an approved file and exchanges it with a complete staged file on macOS APFS.</summary>
    /// <remarks>An absent destination is never created. The revision check and exchange are separate.
    /// Retain the recovery directory and report the actual result after a dispatched write.</remarks>
    public static Task<PresetReplacementOutcome> ReplaceApprovedAsync(PresetReplacementRevision approval, byte[] data,
        int maximumByteCount = 64 * 1024 * 1024, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(approval);
        return DispatchWrite(data, maximumByteCount,
            owned => ReadReplacementOutcome(Native.ReplaceApproved(owned, approval.Token, maximumByteCount)), cancellationToken);
    }

    private static PresetReplacementOutcome ReadReplacementOutcome(byte[] bytes)
    {
        using var result = JsonDocument.Parse(bytes);
        var receipt = result.RootElement;
        if (receipt.ValueKind != JsonValueKind.Object
            || ReadString(receipt, "recoveryDirectory") is not { } directory
            || !Path.IsPathFullyQualified(directory) || directory.Contains('\0'))
            throw InvalidReplacementResult();

        switch (ReadString(receipt, "state"))
        {
            case "committed":
                var identity = ReadString(receipt, "displacedIdentity") switch
                {
                    "matchesApprovedFile" => PresetDisplacedIdentity.MatchesApprovedFile,
                    "different" => PresetDisplacedIdentity.Different,
                    "unverified" => PresetDisplacedIdentity.Unverified,
                    _ => (PresetDisplacedIdentity?)null
                };
                if (identity is { } observed) return new PresetReplacementOutcome.Committed(directory, observed);
                // Preserve the recovery location when the rest of the receipt is incomplete.
                return new PresetReplacementOutcome.CommitUncertain(directory);
            case "commitUncertain":
                return new PresetReplacementOutcome.CommitUncertain(directory);
            default:
                throw InvalidReplacementResult();
        }
    }

    private static string? ReadString(JsonElement element, string name)
        => element.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;

    private static PresetDocumentException InvalidReplacementResult()
    {
        using var error = JsonDocument.Parse("{\"code\":\"nativeFailure\"}");
        return new(error.RootElement);
    }

    private static async Task<T> ObserveAsync<T>(Func<T> operation, CancellationToken cancellationToken)
    {
        try
        {
            var result = await Task.Run(operation, cancellationToken).ConfigureAwait(false);
            cancellationToken.ThrowIfCancellationRequested();
            return result;
        }
        catch { cancellationToken.ThrowIfCancellationRequested(); throw; }
    }

    private static Task<byte[]> WriteAsync(uint operation, byte[] data, string path, int maximumByteCount, CancellationToken cancellationToken)
    {
        var options = new FileOptions(Absolute(path), maximumByteCount);
        return DispatchWrite(data, maximumByteCount,
            owned => Native.Call(operation, owned, options, PresetWireJson.Default.FileOptions), cancellationToken);
    }

    private static Task<T> DispatchWrite<T>(byte[] data, int maximumByteCount, Func<byte[], T> write, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(data);
        if (cancellationToken.IsCancellationRequested)
            return Task.FromCanceled<T>(cancellationToken);

        return PrepareWriteAsync(data, maximumByteCount, write, cancellationToken);
    }

    private static async Task<T> PrepareWriteAsync<T>(byte[] data, int maximumByteCount, Func<byte[], T> write, CancellationToken cancellationToken)
    {
        _ = Native.Call(10, [], new WriteBudget(data.Length, maximumByteCount), PresetWireJson.Default.WriteBudget);
        cancellationToken.ThrowIfCancellationRequested();
        var owned = data.ToArray();
        // Once dispatched, a write reports its actual native result.
        return await Task.Run(() => write(owned), cancellationToken).ConfigureAwait(false);
    }
}
