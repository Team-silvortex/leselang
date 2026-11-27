using System.Collections.Frozen;
using System.Text.Json;

namespace LeseLang.Operations;

public sealed record OperationDefinition(
    string Name,
    IReadOnlyDictionary<string, string> Parameters,
    string Result,
    string Capability,
    Func<IReadOnlyDictionary<string, object?>, OperationContext, ValueTask<object?>> Handle);

/// <summary>Immutable application signatures and their local handlers.</summary>
public sealed class OperationRegistry
{
    private readonly Dictionary<string, OperationDefinition> entries = new(StringComparer.Ordinal);
    private readonly JsonElement description;
    internal IReadOnlySet<string> Capabilities { get; }

    public OperationRegistry(IEnumerable<OperationDefinition> operations)
    {
        ArgumentNullException.ThrowIfNull(operations);
        var schemas = new List<object>();
        foreach (var definition in operations)
        {
            if (entries.Count == 32 || definition is null || !PathName(definition.Name)
                || !PathName(definition.Capability) || !OperationScalars.ValidType(definition.Result)
                || definition.Handle is null || definition.Parameters is null || definition.Parameters.Count > 8)
                throw new ArgumentException("Invalid operation declaration.");

            var parameters = definition.Parameters.ToFrozenDictionary(StringComparer.Ordinal);
            if (parameters.Any(p => !Identifier(p.Key) || !OperationScalars.ValidType(p.Value)))
                throw new ArgumentException("Invalid operation parameter.");
            if (!entries.TryAdd(definition.Name, definition with { Parameters = parameters }))
                throw new ArgumentException("Duplicate operation name.");
            schemas.Add(new { name = definition.Name, parameters, result = definition.Result, capability = definition.Capability });
        }
        if (entries.Count == 0) throw new ArgumentException("At least one operation is required.");
        Capabilities = entries.Values.Select(op => op.Capability).ToFrozenSet(StringComparer.Ordinal);
        description = JsonSerializer.SerializeToElement(schemas);
    }

    internal static bool Identifier(string? name) =>
        name is { Length: >= 1 and <= 64 } && (name[0] == '_' || char.IsAsciiLetter(name[0]))
        && name.All(c => c == '_' || char.IsAsciiLetterOrDigit(c))
        && name is not ("fn" or "true" or "false" or "none" or "body")
        && !name.StartsWith("_lese_", StringComparison.Ordinal);

    private static bool PathName(string? name) => name is { Length: <= 128 }
        && name.Contains('.') && name.Split('.').All(Identifier);

    /// <summary>Operation signatures in declaration order. Grants are selected separately.</summary>
    public JsonElement Describe() => description;

    internal string CapabilityFor(string operation) => entries[operation].Capability;

    internal static void ValidateWireValue(string type, JsonElement value) => OperationScalars.Decode(type, value);

    internal async Task<JsonElement> DispatchAsync(string operation, JsonElement arguments,
        Func<string, bool> allows, CancellationToken cancellation)
    {
        var entry = entries[operation];
        var decoded = new Dictionary<string, object?>(StringComparer.Ordinal);
        foreach (var argument in arguments.EnumerateObject())
        {
            if (!entry.Parameters.TryGetValue(argument.Name, out var type)
                || !decoded.TryAdd(argument.Name, OperationScalars.Decode(type, argument.Value)))
                throw new InvalidDataException("Unexpected operation argument.");
        }
        if (decoded.Count != entry.Parameters.Count) throw new InvalidDataException("Missing operation argument.");

        var context = new OperationContext(cancellation, () => allows(entry.Capability));
        try
        {
            context.CheckAuthority();
            var result = await entry.Handle(decoded.AsReadOnly(), context).ConfigureAwait(false);
            context.Complete();
            var encoded = OperationScalars.Encode(entry.Result, result);
            cancellation.ThrowIfCancellationRequested();
            if (!allows(entry.Capability)) throw new UnauthorizedAccessException("Operation grant revoked.");
            cancellation.ThrowIfCancellationRequested();
            return encoded;
        }
        finally { context.Close(); }
    }
}
