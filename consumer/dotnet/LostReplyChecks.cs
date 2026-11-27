using System.Globalization;
using System.Text.Json;
using LeseLang.Operations;

internal static class LostReplyChecks
{
    public static async Task RunAsync(string executable, string relay)
    {
        using var stream = typeof(LostReplyChecks).Assembly.GetManifestResourceStream("lost-reply.json")
            ?? throw new InvalidOperationException("Missing lost-reply fixture");
        using var document = await JsonDocument.ParseAsync(stream);
        var fixture = document.RootElement;
        var writeSource = fixture.GetProperty("write_source").GetString()!;
        var onceSource = fixture.GetProperty("idempotent_source").GetString()!;
        var lookupSource = fixture.GetProperty("lookup_source").GetString()!;
        var initial = ulong.Parse(fixture.GetProperty("initial_balance").GetString()!, CultureInfo.InvariantCulture);
        var unknownKey = fixture.GetProperty("unknown_key").GetString()!;
        var replay = fixture.GetProperty("replay_control");
        var remaining = new HashSet<string>(["request-lost", "reply-lost"]);
        bool replayed = false;
        foreach (var sample in fixture.GetProperty("cases").EnumerateArray())
        {
            var id = sample.GetProperty("id").GetString()!;
            var key = sample.GetProperty("key").GetString()!;
            if (!remaining.Remove(id) || key == unknownKey) throw new InvalidDataException($"Invalid lost-reply case {id}");
            var arguments = new Dictionary<string, string> { ["key"] = key };
            var ledger = new Ledger(initial, idempotent: false);
            await CheckInterruptedWrite(ledger, writeSource);

            if (id == replay.GetProperty("case").GetString())
            {
                // Explicit negative control: submitting the original write again commits a second time.
                var writer = new OperationHost(executable, ledger.Registry, () => ["ledger.write", "ledger.read"]);
                var report = await writer.ExecuteAsync(writeSource, arguments);
                var balance = ledger.Balance.ToString(CultureInfo.InvariantCulture);
                if (ledger.CommittedWrites != replay.GetProperty("committed_writes").GetInt32()
                    || ledger.AddCalls != ledger.CommittedWrites || ledger.TailReads != 1
                    || balance != replay.GetProperty("balance").GetString() || report.Effects != 2
                    || report.ResultType != "integer" || report.Result.GetString() != balance)
                    throw new InvalidOperationException("Blind replay did not produce the expected second commit");
                replayed = true;
                Console.WriteLine($"Shared blind replay {id}: {ledger.CommittedWrites} committed writes, balance {balance}.");
            }

            var onceLedger = new Ledger(initial, idempotent: true);
            await CheckInterruptedWrite(onceLedger, onceSource);
            await CheckIdempotentRetry(executable, onceLedger, onceSource, lookupSource, sample, fixture.GetProperty("key_isolation"));

            async Task CheckInterruptedWrite(Ledger app, string source)
            {
                var interrupted = new OperationHost(relay, app.Registry, () => ["ledger.write", "ledger.read"]);
                await RequireInjectedCut(interrupted, source, arguments, id);

                // Each lookup starts a fresh real worker; only the application outlives the lost run.
                var reader = new OperationHost(executable, app.Registry, () => ["ledger.lookup"]);
                var receipt = await Lookup(reader, lookupSource, key);
                var unknown = await Lookup(reader, lookupSource, unknownKey);
                var observed = JsonSerializer.SerializeToElement(new
                {
                    run_report = false,
                    add_calls = app.AddCalls,
                    committed_writes = app.CommittedWrites,
                    tail_reads = app.TailReads,
                    lookup_receipt = receipt,
                    unknown_receipt = unknown,
                    balance = app.Balance.ToString(CultureInfo.InvariantCulture)
                });
                if (!app.LookupKeys.SequenceEqual([key, unknownKey])
                    || !JsonElement.DeepEquals(observed, sample.GetProperty("expected")))
                    throw new InvalidOperationException($"{id} observed {observed}");
                var label = source == onceSource ? "idempotent loss" : "lost-reply";
                Console.WriteLine($"Shared {label} {id}: {observed.GetRawText()}");
            }
        }
        if (remaining.Count != 0 || !replayed) throw new InvalidDataException("Missing lost-reply case or replay control");
    }

    private static async Task CheckIdempotentRetry(string executable, Ledger ledger, string source, string lookupSource,
        JsonElement sample, JsonElement isolation)
    {
        var id = sample.GetProperty("id").GetString()!;
        var key = sample.GetProperty("key").GetString()!;
        var writer = new OperationHost(executable, ledger.Registry, () => ["ledger.write", "ledger.read"]);
        var reader = new OperationHost(executable, ledger.Registry, () => ["ledger.lookup"]);

        var retry = await Submit(key);
        var receipt = retry.Result.GetString();
        var observed = JsonSerializer.SerializeToElement(new
        {
            add_calls = ledger.AddCalls,
            committed_writes = ledger.CommittedWrites,
            tail_reads = ledger.TailReads,
            effects = retry.Effects,
            receipt,
            balance = ledger.Balance.ToString(CultureInfo.InvariantCulture)
        });
        if (!JsonElement.DeepEquals(observed, sample.GetProperty("idempotent_retry"))
            || receipt != await Lookup(reader, lookupSource, key))
            throw new InvalidOperationException($"{id} idempotent retry observed {observed}");
        Console.WriteLine($"Shared idempotent retry {id}: {observed.GetRawText()}");

        var otherKey = isolation.GetProperty("other_key").GetString()!;
        if (otherKey == key) throw new InvalidDataException("Key isolation requires distinct business keys");
        var other = await Submit(otherKey);
        var repeated = await Submit(key);
        var isolated = JsonSerializer.SerializeToElement(new
        {
            other_key = otherKey,
            other_receipt = other.Result.GetString(),
            replayed_receipt = repeated.Result.GetString(),
            balance = ledger.Balance.ToString(CultureInfo.InvariantCulture),
            committed_writes = ledger.CommittedWrites
        });
        if (!JsonElement.DeepEquals(isolated, isolation) || repeated.Result.GetString() != receipt)
            throw new InvalidOperationException($"{id} key isolation observed {isolated}");
        Console.WriteLine($"Shared key isolation {id}: {isolated.GetRawText()}");

        async Task<OperationReport> Submit(string writeKey)
        {
            var report = await writer.ExecuteAsync(source, new Dictionary<string, string> { ["key"] = writeKey });
            if (report.Effects != 2 || report.ResultType != "integer")
                throw new InvalidOperationException("Unexpected idempotent write report");
            return report;
        }
    }

    private static async Task<string?> Lookup(OperationHost reader, string source, string key)
    {
        var report = await reader.ExecuteAsync(source, new Dictionary<string, string> { ["key"] = key });
        if (report.Effects != 1 || report.ResultType != "optional_string")
            throw new InvalidOperationException("Unexpected receipt lookup report");
        return report.Result.GetString();
    }

    private static async Task RequireInjectedCut(OperationHost host, string source, Dictionary<string, string> arguments, string id)
    {
        // OperationHost drains worker stderr. The relay independently names the cut it actually reached.
        const string variable = "LESE_TEST_CUT_LOG";
        var previous = Environment.GetEnvironmentVariable(variable);
        var marker = Path.GetTempFileName();
        try
        {
            Environment.SetEnvironmentVariable(variable, marker);
            try { await host.ExecuteAsync(source, arguments); }
            catch (EndOfStreamException)
            {
                if (!(await File.ReadAllLinesAsync(marker)).SequenceEqual([$"lese-test-cut {id}"]))
                    throw new InvalidDataException($"Worker EOF without the expected {id} cut");
                return;
            }
            throw new InvalidOperationException($"The {id} cut delivered a run report");
        }
        finally
        {
            Environment.SetEnvironmentVariable(variable, previous);
            File.Delete(marker);
        }
    }

    private sealed class Ledger
    {
        private readonly Dictionary<string, ulong> receipts = new(StringComparer.Ordinal);
        public ulong Balance { get; private set; }
        public int AddCalls { get; private set; }
        public int CommittedWrites { get; private set; }
        public int TailReads { get; private set; }
        public List<string> LookupKeys { get; } = [];
        public OperationRegistry Registry { get; }

        public Ledger(ulong initial, bool idempotent)
        {
            Balance = initial;
            var writeOperation = idempotent ? "ledger.add_once" : "ledger.add";
            var key = new Dictionary<string, string> { ["key"] = "string" };
            Registry = new OperationRegistry([
                new(writeOperation, key, "integer", "ledger.write", (args, context) =>
                {
                    var writeKey = (string)args["key"]!;
                    lock (receipts)
                    {
                        AddCalls++;
                        if (idempotent && receipts.TryGetValue(writeKey, out var saved))
                            return ValueTask.FromResult<object?>(saved);
                        var receipt = checked(Balance + 1);
                        context.Commit(() =>
                        {
                            receipts[writeKey] = receipt;
                            Balance = receipt;
                            CommittedWrites++;
                        });
                        return ValueTask.FromResult<object?>(receipt);
                    }
                }),
                new("ledger.read", new Dictionary<string, string>(), "integer", "ledger.read", (_, _) =>
                {
                    lock (receipts)
                    {
                        TailReads++;
                        return ValueTask.FromResult<object?>(Balance);
                    }
                }),
                new("ledger.lookup", key, "optional_string", "ledger.lookup", (args, _) =>
                {
                    var lookupKey = (string)args["key"]!;
                    lock (receipts)
                    {
                        LookupKeys.Add(lookupKey);
                        return ValueTask.FromResult<object?>(receipts.TryGetValue(lookupKey, out var receipt)
                            ? receipt.ToString(CultureInfo.InvariantCulture) : null);
                    }
                })
            ]);
        }
    }
}
