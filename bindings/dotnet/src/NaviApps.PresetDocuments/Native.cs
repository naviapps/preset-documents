using System.Buffers;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace NaviApps.PresetDocuments;

internal static class Native
{
    private const string Library = "preset_documents";
    internal static readonly JsonSerializerOptions Json = new() { PropertyNamingPolicy = JsonNamingPolicy.CamelCase };
    static Native() => NativeLibrary.SetDllImportResolver(typeof(Native).Assembly, Resolve);
    private static IntPtr Resolve(string name, Assembly assembly, DllImportSearchPath? search)
    {
        if (name != Library) return IntPtr.Zero;
        var explicitPath = Environment.GetEnvironmentVariable("PRESET_DOCUMENTS_NATIVE_LIBRARY");
        if (explicitPath is not null) return NativeLibrary.Load(explicitPath);
        string file = OperatingSystem.IsWindows() ? Library + ".dll" : "lib" + Library + ".dylib";
        string packaged = Path.Combine(AppContext.BaseDirectory, "runtimes", RuntimeInformation.RuntimeIdentifier, "native", file);
        return File.Exists(packaged) ? NativeLibrary.Load(packaged) : NativeLibrary.Load(file, assembly, search);
    }
    [DllImport(Library, EntryPoint = "preset_documents_abi_version", CallingConvention = CallingConvention.Cdecl)]
    private static extern uint AbiVersion();
    [DllImport(Library, EntryPoint = "preset_documents_process", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr Process(uint operation, byte[] data, nuint size, byte[] options, nuint optionsSize);
    [DllImport(Library, EntryPoint = "preset_documents_buffer_data", CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr Data(IntPtr owner, out nuint size, out uint status);
    [DllImport(Library, EntryPoint = "preset_documents_buffer_free", CallingConvention = CallingConvention.Cdecl)]
    private static extern void Free(IntPtr owner);

    internal static byte[] Call(uint operation, byte[] data, object? options = null)
        => Call(operation, data, data.Length, options);

    internal static byte[] CallWritten(uint operation, ArrayBufferWriter<byte> writer, object options)
    {
        // The synchronous native call reads only the written prefix, never the spare capacity.
        if (MemoryMarshal.TryGetArray(writer.WrittenMemory, out var segment)
            && segment.Offset == 0 && segment.Array is { } buffer)
            return Call(operation, buffer, segment.Count, options);
        return Call(operation, writer.WrittenSpan.ToArray(), options);
    }

    private static byte[] Call(uint operation, byte[] data, int length, object? options)
    {
        if (AbiVersion() != 1) throw new InvalidOperationException("Native preset ABI mismatch.");
        var config = options is null ? Array.Empty<byte>() : JsonSerializer.SerializeToUtf8Bytes(options, Json);
        var owner = Process(operation, data, (nuint)length, config, (nuint)config.Length);
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
