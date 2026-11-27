using System.Globalization;
using System.Text;
using System.Text.Json;

namespace LeseLang.Operations;

internal static class OperationScalars
{
    private static readonly UTF8Encoding Utf8 = new(false, true);

    internal static bool ValidType(string type) =>
        type is "integer" or "boolean" or "string" or "none" or "optional_string" or "string_list";

    private static string Text(string value)
    {
        if (value.Length > 4096 || Utf8.GetByteCount(value) > 4096) throw Invalid();
        return value;
    }

    private static InvalidDataException Invalid() => new("Value does not match the declared bounded Lese type.");

    private static IReadOnlyList<string> CopyList(IReadOnlyList<string> values)
    {
        if (values.Count > 64) throw Invalid();
        var copy = new string[values.Count];
        var bytes = 0;
        for (var i = 0; i < copy.Length; i++)
        {
            var item = values[i] ?? throw Invalid();
            if (item.Length > 4096 || (bytes += Utf8.GetByteCount(item)) > 4096) throw Invalid();
            copy[i] = item;
        }
        return Array.AsReadOnly(copy);
    }

    internal static object? Decode(string type, JsonElement value)
    {
        switch (type, value.ValueKind)
        {
            case ("integer", JsonValueKind.String):
                var digits = value.GetString()!;
                if (digits.Length <= 20 && ulong.TryParse(digits, NumberStyles.None, CultureInfo.InvariantCulture, out var integer)
                    && digits == integer.ToString(CultureInfo.InvariantCulture)) return integer;
                break;
            case ("string" or "optional_string", JsonValueKind.String): return Text(value.GetString()!);
            case ("none" or "optional_string", JsonValueKind.Null): return null;
            case ("boolean", JsonValueKind.True or JsonValueKind.False): return value.GetBoolean();
            case ("string_list", JsonValueKind.Array) when value.GetArrayLength() <= 64:
                return CopyList(value.EnumerateArray().Select(x => x.GetString() ?? throw Invalid()).ToArray());
        }
        throw Invalid();
    }

    internal static JsonElement Encode(string type, object? value)
    {
        object? wire = (type, value) switch
        {
            ("integer", ulong number) => number.ToString(CultureInfo.InvariantCulture),
            ("string" or "optional_string", string text) => Text(text),
            ("none" or "optional_string", null) => null,
            ("boolean", bool boolean) => boolean,
            ("string_list", IReadOnlyList<string> items) => CopyList(items),
            _ => throw Invalid()
        };
        return JsonSerializer.SerializeToElement(wire);
    }
}
