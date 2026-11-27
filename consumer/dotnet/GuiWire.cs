using System.Globalization;
using System.Text;
using System.Text.Json;

// Framing for this independent GUI Consumer. BufferedStream keeps bytes after LF for the next read.
internal sealed class GuiWire(Stream stream) : IDisposable
{
    internal const int RequestLimit = 64 * 1024;
    internal static readonly UTF8Encoding Utf8 = new(false, true);
    private static readonly JsonDocumentOptions StrictJson = new() { AllowDuplicateProperties = false, MaxDepth = 32 };
    private readonly BufferedStream input = new(stream);
    private readonly byte[] next = new byte[1];

    internal async Task<byte[]> ReadFrameAsync(CancellationToken token)
    {
        using var line = new MemoryStream();
        while (line.Length < 512 * 1024)
        {
            if (await input.ReadAsync(next, token) == 0) throw new EndOfStreamException("GUI frame ended before LF");
            line.WriteByte(next[0]);
            if (next[0] == (byte)'\n') return line.ToArray();
        }
        throw new InvalidDataException("Oversized GUI worker frame");
    }

    internal async Task RequireEndAsync(CancellationToken token)
    {
        if (await input.ReadAsync(next, token) != 0) throw new InvalidDataException("Unexpected trailing GUI worker output");
    }

    internal static async Task WriteFrameAsync(Stream output, object value, int limit, CancellationToken token)
    {
        var frame = JsonSerializer.SerializeToUtf8Bytes(value);
        if (frame.Length >= limit) throw new InvalidDataException("Oversized GUI worker input");
        await output.WriteAsync(frame, token);
        await output.WriteAsync("\n"u8.ToArray(), token);
        await output.FlushAsync(token);
    }

    internal static JsonDocument Parse(byte[] bytes) => JsonDocument.Parse(Utf8.GetString(bytes), StrictJson);

    internal static void Fields(JsonElement value, params ReadOnlySpan<string> names)
    {
        if (value.ValueKind != JsonValueKind.Object || value.GetPropertyCount() != names.Length)
            throw new InvalidDataException("Unexpected GUI wire fields");
        foreach (var name in names)
            if (!value.TryGetProperty(name, out _)) throw new InvalidDataException("Missing GUI wire field " + name);
    }

    internal static int Number(JsonElement value, string name)
    {
        var property = value.GetProperty(name);
        if (!property.TryGetInt32(out int number) || number < 0 || property.GetRawText() != number.ToString(CultureInfo.InvariantCulture))
            throw new InvalidDataException("Invalid GUI integer " + name);
        return number;
    }

    internal static string Text(JsonElement value, string name) =>
        value.GetProperty(name).GetString() ?? throw new InvalidDataException("Missing GUI text " + name);

    public void Dispose() => input.Dispose();
}
