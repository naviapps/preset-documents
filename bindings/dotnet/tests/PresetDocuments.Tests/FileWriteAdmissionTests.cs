using System.Diagnostics;
using NaviApps.PresetDocuments;

internal static class FileWriteAdmissionTests
{
    internal static async Task RunAsync()
    {
        var directory = Directory.CreateTempSubdirectory("preset-write-admission-");
        try
        {
            var existing = Path.Combine(directory.FullName, "existing.preset");
            var missing = Path.Combine(directory.FullName, "missing.preset");
            byte[] original = "keep this preset"u8.ToArray();
            await File.WriteAllBytesAsync(existing, original);
            var payload = new byte[8 * 1024 * 1024];

            using var cancellation = new CancellationTokenSource();
            cancellation.Cancel();
            var token = cancellation.Token;
            var requests = new List<(string Operation, Func<int, CancellationToken, Task> Start)>
            {
                ("create-only", (maximum, ct) => PresetDocumentFiles.WriteNewAsync(payload, missing, maximum, ct)),
                ("atomic replace", (maximum, ct) => PresetDocumentFiles.WriteAtomicallyAsync(payload, existing, maximum, ct)),
            };
            try
            {
                var approval = await PresetDocumentFiles.CaptureReplacementAsync(existing);
                requests.Add(("approved replace", (maximum, ct) => PresetDocumentFiles.ReplaceApprovedAsync(approval, payload, maximum, ct)));
            }
            catch (PresetDocumentException error) when (error.Code == PresetDocumentErrorCode.ApprovedReplacementUnsupported) { }

            var copying = new List<string>();
            long maximumAllocation = 0;
            foreach (var (operation, start) in requests)
            {
                long before = GC.GetAllocatedBytesForCurrentThread();
                Task write = start(3, token);
                long allocated = GC.GetAllocatedBytesForCurrentThread() - before;
                maximumAllocation = Math.Max(maximumAllocation, allocated);
                if (allocated >= payload.Length / 2)
                    copying.Add($"{operation} allocated {allocated} bytes");

                try
                {
                    await write;
                    throw new UnreachableException($"The pre-cancelled {operation} succeeded.");
                }
                catch (OperationCanceledException error) when (error.CancellationToken == token) { }
            }
            if (copying.Count > 0)
                throw new UnreachableException($"Pre-cancelled writes copied an unneeded payload: {string.Join(", ", copying)}.");
            Console.WriteLine($"Pre-cancelled writes avoided payload copies across {requests.Count} file operations. Largest request allocation was {maximumAllocation} bytes.");

            maximumAllocation = 0;
            foreach (var (operation, start) in requests)
            {
                foreach (int maximum in new[] { 0, 3 })
                {
                    long before = GC.GetAllocatedBytesForCurrentThread();
                    Task write = start(maximum, default);
                    long allocated = GC.GetAllocatedBytesForCurrentThread() - before;
                    maximumAllocation = Math.Max(maximumAllocation, allocated);
                    if (allocated >= payload.Length / 2)
                        copying.Add($"{operation} with limit {maximum} allocated {allocated} bytes");

                    try
                    {
                        await write;
                        throw new UnreachableException($"The {operation} with limit {maximum} succeeded.");
                    }
                    catch (PresetDocumentException error) when (maximum == 0 && error.Code == PresetDocumentErrorCode.InvalidConfiguration) { }
                    catch (PresetDocumentException error) when (maximum == 3 && error.Code == PresetDocumentErrorCode.OutputTooLarge)
                    {
                        if (error.Details.GetProperty("byteCount").GetInt32() != payload.Length
                            || error.Details.GetProperty("maximumByteCount").GetInt32() != maximum)
                            throw new UnreachableException($"The {operation} reported the wrong write budget.");
                    }
                }
            }
            if (File.Exists(missing) || !File.ReadAllBytes(existing).SequenceEqual(original)
                || directory.GetFileSystemInfos().Length != 1)
                throw new UnreachableException("A rejected write changed its destination or left a staged file.");
            if (copying.Count > 0)
                throw new UnreachableException($"Rejected writes copied an unneeded payload: {string.Join(", ", copying)}.");
            Console.WriteLine($"Rejected write budgets avoided payload copies across {requests.Count} file operations. Largest request allocation was {maximumAllocation} bytes.");
        }
        finally { directory.Delete(recursive: true); }
    }
}
