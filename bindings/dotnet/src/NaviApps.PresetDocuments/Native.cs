using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Serialization.Metadata;

namespace NaviApps.PresetDocuments;

internal static partial class Native
{
    private const string Library = "preset_documents";
    [LibraryImport(Library, EntryPoint = "preset_documents_abi_version")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial uint AbiVersion();
    [LibraryImport(Library, EntryPoint = "preset_documents_process")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static unsafe partial IntPtr Process(uint operation, byte* data, nuint size, byte* options, nuint optionsSize);
    [LibraryImport(Library, EntryPoint = "preset_documents_replace_approved")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static unsafe partial IntPtr ReplaceApproved(byte* data, nuint size, byte* revision, nuint revisionSize, nuint maximumByteCount);
    [LibraryImport(Library, EntryPoint = "preset_documents_buffer_data")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial IntPtr Data(IntPtr owner, out nuint size, out uint status);
    [LibraryImport(Library, EntryPoint = "preset_documents_buffer_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial void Free(IntPtr owner);

    internal static byte[] Call<T>(uint operation, ReadOnlySpan<byte> data, T options, JsonTypeInfo<T> typeInfo)
        => Call(operation, data, JsonSerializer.SerializeToUtf8Bytes(options, typeInfo));

    internal static unsafe byte[] Call(uint operation, ReadOnlySpan<byte> data, ReadOnlySpan<byte> options = default)
    {
        RequireMatchingAbi();
        IntPtr owner;
        // The native call borrows the spans only until it returns.
        fixed (byte* input = data, configuration = options)
            owner = Process(operation, input, (nuint)data.Length, configuration, (nuint)options.Length);
        return ReadResult(owner);
    }

    internal static unsafe byte[] ReplaceApproved(ReadOnlySpan<byte> data, ReadOnlySpan<byte> revision, int maximumByteCount)
    {
        RequireMatchingAbi();
        IntPtr owner;
        fixed (byte* input = data, token = revision)
            owner = ReplaceApproved(input, (nuint)data.Length, token, (nuint)revision.Length, (nuint)maximumByteCount);
        return ReadResult(owner);
    }

    private static void RequireMatchingAbi()
    {
        if (AbiVersion() != 2) throw new InvalidOperationException("Native preset ABI mismatch.");
    }

    private static byte[] ReadResult(IntPtr owner)
    {
        if (owner == IntPtr.Zero) throw new InvalidOperationException("Missing native result.");
        try
        {
            var pointer = Data(owner, out var size, out var status);
            if (pointer == IntPtr.Zero || size > 64 * 1024 * 1024) throw new InvalidOperationException("Invalid native result.");
            byte[] bytes = new byte[checked((int)size)];
            Marshal.Copy(pointer, bytes, 0, bytes.Length);
            if (status == 1) { using var error = JsonDocument.Parse(bytes); throw new PresetDocumentException(error.RootElement); }
            if (status != 0) throw new InvalidOperationException("Unknown native result status.");
            return bytes;
        }
        finally { Free(owner); }
    }
}
