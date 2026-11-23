using System.Diagnostics;
using NaviApps.PresetDocuments;

internal static class ApprovedReplacementTests
{
    internal static async Task RunAsync()
    {
        try
        {
            await PresetDocumentFiles.CaptureReplacementAsync("relative.preset");
            throw new UnreachableException("An approval captured a relative path.");
        }
        catch (ArgumentException) { }

        var root = Directory.CreateTempSubdirectory("preset-approval-");
        try
        {
            var file = Path.Combine(root.FullName, OperatingSystem.IsWindows() ? "承認済み.preset" : "承認済み \"export\".preset");
            var original = "approved original"u8.ToArray();
            var replacement = "replacement"u8.ToArray();
            await File.WriteAllBytesAsync(file, original);

            var replacementCharacter = Path.Combine(root.FullName, "\uFFFD.preset");
            await File.WriteAllBytesAsync(replacementCharacter, original);
            foreach (var alternate in new[] { Path.Combine(root.FullName, "\uD800.preset"), file + Path.DirectorySeparatorChar })
            {
                await RequireInvalidPathAsync(() => PresetDocumentFiles.WriteAtomicallyAsync(replacement, alternate));
                await RequireInvalidPathAsync(() => PresetDocumentFiles.CaptureReplacementAsync(alternate));
            }
            Require(File.ReadAllBytes(file).SequenceEqual(original));
            Require((await PresetDocumentFiles.ReadAsync(replacementCharacter)).SequenceEqual(original));
            File.Delete(replacementCharacter);

            using var cancelled = new CancellationTokenSource();
            cancelled.Cancel();
            await RequireCancellationAsync(() => PresetDocumentFiles.CaptureReplacementAsync(file, cancelled.Token));

            PresetReplacementRevision approval;
            try { approval = await PresetDocumentFiles.CaptureReplacementAsync(file); }
            catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.ApprovedReplacementUnsupported)
            {
                Require(File.ReadAllBytes(file).SequenceEqual(original) && root.GetFileSystemInfos().Length == 1);
                Console.WriteLine("Approved replacement returned the typed unsupported error without changing the destination.");
                return;
            }
            Require(OperatingSystem.IsMacOS());

            foreach (var count in new[] { replacement.Length, 64 * 1024 * 1024 + 1 })
            {
                var limitError = await RequirePresetErrorAsync(PresetDocumentErrorCode.OutputTooLarge,
                    () => PresetDocumentFiles.ReplaceApprovedAsync(approval, new byte[count], maximumByteCount: 1));
                Require(limitError.Details.GetProperty("byteCount").GetInt32() == count);
                Require(limitError.Details.GetProperty("maximumByteCount").GetInt32() == 1);
            }
            await RequireCancellationAsync(() => PresetDocumentFiles.ReplaceApprovedAsync(approval, replacement, cancellationToken: cancelled.Token));
            Require(File.ReadAllBytes(file).SequenceEqual(original) && root.GetFileSystemInfos().Length == 1);

            var former = Path.Combine(root.FullName, "former.preset");
            File.Move(file, former);
            var competing = "competing destination"u8.ToArray();
            await File.WriteAllBytesAsync(file, competing);
            await RequirePresetErrorAsync(PresetDocumentErrorCode.DestinationChanged,
                () => PresetDocumentFiles.ReplaceApprovedAsync(approval, replacement));
            Require(File.ReadAllBytes(file).SequenceEqual(competing));
            Require(File.ReadAllBytes(former).SequenceEqual(original));

            var renewed = await PresetDocumentFiles.CaptureReplacementAsync(file);
            var outcome = await PresetDocumentFiles.ReplaceApprovedAsync(renewed, replacement, replacement.Length);
            Require(outcome is PresetReplacementOutcome.Committed { DisplacedIdentity: PresetDisplacedIdentity.MatchesApprovedFile });
            Require(Directory.Exists(outcome.RecoveryDirectory));
            Require(Path.GetFileName(outcome.RecoveryDirectory) == root.GetDirectories().Single().Name);
            Require(Path.GetFileName(outcome.DisplacedPresetPath) == "displaced-preset");
            Require(File.ReadAllBytes(outcome.DisplacedPresetPath).SequenceEqual(competing));
            Require(File.ReadAllBytes(file).SequenceEqual(replacement));
            Console.WriteLine("Approved replacement preserved stale, undersized and cancelled destinations and retained the displaced preset on commit.");
        }
        finally { root.Delete(true); }
    }

    private static void Require(bool condition)
    {
        if (!condition) throw new UnreachableException("Approved replacement contract failed.");
    }

    private static async Task RequireCancellationAsync(Func<Task> operation)
    {
        try { await operation(); }
        catch (OperationCanceledException) { return; }
        throw new UnreachableException("Approved replacement ignored pre-dispatch cancellation.");
    }

    private static async Task RequireInvalidPathAsync(Func<Task> operation)
    {
        try { await operation(); }
        catch (ArgumentException) { return; }
        throw new UnreachableException("An ambiguous path was accepted.");
    }

    private static async Task<PresetDocumentException> RequirePresetErrorAsync(PresetDocumentErrorCode code, Func<Task> operation)
    {
        try { await operation(); }
        catch (PresetDocumentException error) when (error.Code == code) { return error; }
        throw new UnreachableException($"Approved replacement did not return {code}.");
    }
}
