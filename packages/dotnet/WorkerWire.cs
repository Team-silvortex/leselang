using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace LeseLang.Operations;

// One bounded byte reader owns stdout, including any bytes following the LF.
// PeekAsync starts a read without consuming the first byte of the next frame.
internal sealed class WorkerWire(Stream input)
{
    internal const int RequestLimit = 64 * 1024;
    private const int FrameLimit = 512 * 1024;
    internal static readonly UTF8Encoding Utf8 = new(false, true);
    private static readonly JsonDocumentOptions StrictJson = new() { AllowDuplicateProperties = false, MaxDepth = 32 };
    private static readonly JsonSerializerOptions OutputJson = new() { Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping };
    private readonly byte[] buffer = new byte[4096];
    private int offset, end;

    internal async Task<bool> PeekAsync(CancellationToken token)
    {
        if (offset < end) return true;
        offset = 0;
        end = await input.ReadAsync(buffer, token).ConfigureAwait(false);
        return end != 0;
    }

    internal async Task<byte[]> ReadFrameAsync(CancellationToken token)
    {
        using var frame = new MemoryStream();
        while (await PeekAsync(token).ConfigureAwait(false))
        {
            int lf = Array.IndexOf(buffer, (byte)'\n', offset, end - offset);
            int count = (lf < 0 ? end : lf + 1) - offset;
            if (frame.Length + count > FrameLimit) throw new InvalidDataException("Oversized worker frame");
            frame.Write(buffer, offset, count);
            offset += count;
            if (lf >= 0) return frame.ToArray();
        }
        throw new EndOfStreamException("Worker closed without a complete frame");
    }

    internal static JsonDocument Parse(byte[] bytes)
    {
        Utf8.GetCharCount(bytes);
        return JsonDocument.Parse(bytes, StrictJson);
    }

    internal static byte[] Encode(object value, int limit)
    {
        var bytes = JsonSerializer.SerializeToUtf8Bytes(value, OutputJson);
        if (bytes.Length + 1 > limit) throw new InvalidDataException("Oversized worker input");
        Array.Resize(ref bytes, bytes.Length + 1);
        bytes[^1] = (byte)'\n';
        return bytes;
    }

    internal static async Task SendAsync(Stream stdin, byte[] frame, CancellationToken token)
    {
        // Process can buffer redirected stdin on Windows. Deliver this complete
        // record before waiting for the corresponding worker output.
        await stdin.WriteAsync(frame, token).ConfigureAwait(false);
        await stdin.FlushAsync(token).ConfigureAwait(false);
    }

    internal static void Fields(JsonElement value, params ReadOnlySpan<string> names)
    {
        if (value.ValueKind != JsonValueKind.Object || value.GetPropertyCount() != names.Length)
            throw new InvalidDataException("Invalid worker record");
        foreach (string name in names)
            if (!value.TryGetProperty(name, out _)) throw new InvalidDataException("Missing worker field " + name);
    }

    internal static int Number(JsonElement value, string name)
    {
        var field = value.GetProperty(name);
        if (field.ValueKind != JsonValueKind.Number || !field.TryGetInt32(out int n) || n < 0
            || field.GetRawText() != n.ToString(System.Globalization.CultureInfo.InvariantCulture))
            throw new InvalidDataException("Invalid worker integer " + name);
        return n;
    }

    internal static string Text(JsonElement value, string name) =>
        value.GetProperty(name).GetString() ?? throw new InvalidDataException("Invalid worker text " + name);
}
