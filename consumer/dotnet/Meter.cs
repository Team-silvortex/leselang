using LeseLang.Operations;

// Application-owned state and policy. The only changes to the balance go through the Effect's commit gate.
internal sealed class Meter
{
    public const ulong Initial = 9_007_199_254_740_992;
    private volatile bool canWrite = true;
    public bool CanWrite { get => canWrite; set => canWrite = value; }
    public bool BadWriteResult { get; set; }
    public Func<OperationContext, Task>? BeforeCommit { get; set; }
    public Action? WriteFinished { get; set; }
    public ulong Balance { get; private set; } = Initial;
    public int AddCalls { get; private set; }
    public int ReadCalls { get; private set; }
    public OperationRegistry Registry { get; }

    public Meter()
    {
        Registry = new OperationRegistry([
            new("meter.add", new Dictionary<string, string> { ["amount"] = "integer" },
                "integer", "meter.write", Add),
            new("meter.read", new Dictionary<string, string>(), "integer", "meter.read",
                (_, _) =>
                {
                    ReadCalls++;
                    return ValueTask.FromResult<object?>(Balance);
                })
        ]);
    }

    public IReadOnlyCollection<string> Grants() => CanWrite ? ["meter.write", "meter.read"] : ["meter.read"];
    public OperationHost Connect(string executable) => new(executable, Registry, Grants);

    private async ValueTask<object?> Add(IReadOnlyDictionary<string, object?> args, OperationContext scope)
    {
        AddCalls++;
        try
        {
            if (BeforeCommit is not null) await BeforeCommit(scope);
            var amount = (ulong)args["amount"]!;
            scope.Commit(() => Balance = checked(Balance + amount));
            return BadWriteResult ? "wrong type" : Balance;
        }
        finally { WriteFinished?.Invoke(); }
    }
}
