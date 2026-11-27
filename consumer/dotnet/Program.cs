using System.Globalization;
using System.Runtime.InteropServices;
using System.Text.Json;
using LeseLang.Operations;

if (args is ["--serve"]) return await ProtocolChecks.ServeAsync();
if (args.Length != 2)
{
    Console.Error.WriteLine("Usage: LeseLang.Consumer <absolute worker path> <two-effects.json>");
    return 2;
}

var executable = args[0];
Console.WriteLine($"C# Consumer on {RuntimeInformation.OSDescription} ({RuntimeInformation.ProcessArchitecture}), .NET {Environment.Version}");
using var fixture = JsonDocument.Parse(await File.ReadAllBytesAsync(args[1]));
var meter = new Meter();
var host = meter.Connect(executable);
var catalog = host.Describe();
Check(catalog.GetProperty("profile").GetString() == "lese.operations"
    && catalog.GetProperty("schema_version").GetInt32() == 1, "unexpected operation profile");
var declarations = catalog.GetProperty("operations");
var contract = fixture.RootElement;
Check(JsonElement.DeepEquals(declarations, contract.GetProperty("operations")), "unexpected operation signatures");
Check(meter.Grants().ToHashSet().SetEquals(contract.GetProperty("grants").EnumerateArray().Select(g => g.GetString()!)),
    "the application grants differ from the fixture");
var source = Plan(declarations, "meter.add", "meter.read", 1);
Check(source == contract.GetProperty("source").GetString(), "provider generated a different program");
var inputs = contract.GetProperty("arguments").EnumerateObject().ToDictionary(p => p.Name, p => p.Value.GetString()!);

var before = meter.Balance;
var report = await host.ExecuteAsync(source, inputs);
var expected = contract.GetProperty("expected");
var result = expected.GetProperty("result");
Check(report.ResultType == result.GetProperty("type").GetString()
    && JsonElement.DeepEquals(report.Result, result.GetProperty("value"))
    && report.Effects == expected.GetProperty("effects").GetInt32()
    && report.FuelRemaining == expected.GetProperty("fuel_remaining").GetUInt64(),
    "the worker report differs from the shared fixture");
Check(meter.Balance == before + 1 && report.Result.GetString() == meter.Balance.ToString(CultureInfo.InvariantCulture)
    && meter.AddCalls == 1 && meter.ReadCalls == 1, "the application did not observe the requested increment");
Console.WriteLine($"Verified two Effects. Balance {meter.Balance}, Fuel remaining {report.FuelRemaining}.");

await CheckMissingGrant();
Console.WriteLine("Passed missing grant: zero callbacks, subsequent run verified.");
await CheckPartialWrite();
Console.WriteLine("Passed partial write: first mutation retained once, subsequent read skipped.");
await CheckAwaitedWrite(cancel: false);
Console.WriteLine("Passed revocation during await: pending write and stale context fenced, subsequent run verified.");
await CheckAwaitedWrite(cancel: true);
Console.WriteLine("Passed cancellation during await: pending write and stale context fenced, subsequent run verified.");
await CheckSwallowedDenial();
Console.WriteLine("Passed swallowed authorization denial: context stayed retired after regrant.");
await CheckScalarRoundTrips();
Console.WriteLine("Passed six scalar types, including exact u64 maximum and optional-string absence.");
await ProtocolChecks.RunAsync();
Console.WriteLine("C# Consumer passed grant, partial-effect, revocation, cancellation, scalar and worker framing checks.");
return 0;

// This offline provider takes two selected operations and reads their argument names
// from discovery. The checked unsigned integer is the only literal emitted into source.
static string Plan(JsonElement catalog, string increment, string read, ulong amount)
{
    var writeOp = catalog.EnumerateArray().Single(op => op.GetProperty("name").GetString() == increment);
    var readOp = catalog.EnumerateArray().Single(op => op.GetProperty("name").GetString() == read);
    var parameter = writeOp.GetProperty("parameters").EnumerateObject().Single();
    Check(parameter.Value.GetString() == "integer" && writeOp.GetProperty("result").GetString() == "integer"
        && readOp.GetProperty("result").GetString() == "integer"
        && !readOp.GetProperty("parameters").EnumerateObject().Any(), "unsupported command signatures");
    return $"fn main() = bind(added: {increment}({parameter.Name}: {amount.ToString(CultureInfo.InvariantCulture)}), body: {read}())";
}

async Task CheckMissingGrant()
{
    var app = new Meter { CanWrite = false };
    var runner = app.Connect(executable);
    await MustFail<InvalidDataException>(runner.ExecuteAsync(source, inputs));
    Check(app.AddCalls == 0 && app.ReadCalls == 0 && app.Balance == Meter.Initial, "grant denial reached a handler");
    app.CanWrite = true;
    await CheckFreshRun(runner, app, Meter.Initial);
}

async Task CheckPartialWrite()
{
    var app = new Meter { BadWriteResult = true };
    var runner = app.Connect(executable);
    await MustFail<InvalidDataException>(runner.ExecuteAsync(source, inputs));
    Check(app.AddCalls == 1 && app.ReadCalls == 0 && app.Balance == Meter.Initial + 1,
        "a malformed reply lost, duplicated or followed a committed write");
    app.BadWriteResult = false;
    await CheckFreshRun(runner, app, Meter.Initial + 1);
}

async Task CheckAwaitedWrite(bool cancel)
{
    var app = new Meter();
    var runner = app.Connect(executable);
    var entered = new TaskCompletionSource<OperationContext>(TaskCreationOptions.RunContinuationsAsynchronously);
    var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    var settled = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    app.BeforeCommit = scope =>
    {
        entered.TrySetResult(scope);
        return release.Task;
    };
    app.WriteFinished = () => settled.TrySetResult();
    using var cancellation = new CancellationTokenSource();
    var run = runner.ExecuteAsync(source, inputs, cancellation.Token);
    OperationContext stale;
    try
    {
        if (await Task.WhenAny(entered.Task, run).WaitAsync(TimeSpan.FromSeconds(10)) == run)
        {
            await run;
            throw new InvalidOperationException("worker completed without entering the write");
        }
        stale = await entered.Task;
        if (cancel)
        {
            cancellation.Cancel();
            await MustFail<OperationCanceledException>(run);
        }
        else app.CanWrite = false;
    }
    finally { release.TrySetResult(); }
    if (!cancel) await MustFail<UnauthorizedAccessException>(run);
    await settled.Task.WaitAsync(TimeSpan.FromSeconds(10));
    Check(app.Balance == Meter.Initial && app.AddCalls == 1 && app.ReadCalls == 0, "the stopped handler committed");

    app.CanWrite = true;
    app.BeforeCommit = null;
    await CheckFreshRun(runner, app, Meter.Initial);
    var lateWrites = 0;
    await MustFail<InvalidOperationException>(Task.Run(() => stale.Commit(() => lateWrites++)));
    Check(lateWrites == 0, "old context acquired authority in a later run");
}

async Task CheckFreshRun(OperationHost runner, Meter app, ulong startingBalance)
{
    var addCalls = app.AddCalls;
    var readCalls = app.ReadCalls;
    var next = await runner.ExecuteAsync(source, inputs);
    Check(next.Effects == 2 && next.ResultType == "integer"
        && next.Result.GetString() == (startingBalance + 1).ToString(CultureInfo.InvariantCulture)
        && app.Balance == startingBalance + 1 && app.AddCalls == addCalls + 1 && app.ReadCalls == readCalls + 1,
        "new run did not commit and verify exactly once");
}

async Task CheckSwallowedDenial()
{
    var canWrite = true;
    var denials = 0;
    var writes = 0;
    var registry = new OperationRegistry([
        new("test.update", new Dictionary<string, string>(), "boolean", "test.write", (_, context) =>
        {
            canWrite = false;
            try { context.Commit(() => writes++); }
            catch (UnauthorizedAccessException) { denials++; }
            finally { canWrite = true; }
            return ValueTask.FromResult<object?>(true);
        })
    ]);
    var runner = new OperationHost(executable, registry, () => canWrite ? ["test.write"] : []);
    await MustFail<InvalidOperationException>(runner.ExecuteAsync("fn main() = test.update()", inputs));
    Check(denials == 1 && writes == 0, "a swallowed denial restored authority to the same handler");
}

async Task CheckScalarRoundTrips()
{
    foreach (var (type, value) in new (string, object?)[] {
        ("integer", ulong.MaxValue), ("boolean", false), ("string", "备件"), ("none", null),
        ("optional_string", null), ("optional_string", ""), ("string_list", new[] { "一", "two" })
    })
    {
        var registry = new OperationRegistry([
            new("sample.produce", new Dictionary<string, string>(), type, "sample.read",
                (_, _) => ValueTask.FromResult(value)),
            new("sample.echo", new Dictionary<string, string> { ["item"] = type }, type, "sample.read",
                (arguments, _) => ValueTask.FromResult(arguments["item"]))
        ]);
        var runner = new OperationHost(executable, registry, () => ["sample.read"]);
        var completed = await runner.ExecuteAsync("fn main() = bind(value: sample.produce(), body: sample.echo(item: value))", inputs);
        var wire = JsonSerializer.SerializeToElement(value is ulong n ? n.ToString(CultureInfo.InvariantCulture) : value);
        Check(completed.ResultType == type && completed.Effects == 2 && JsonElement.DeepEquals(completed.Result, wire),
            $"wrong {type} round trip");
    }
}

static async Task MustFail<TException>(Task task) where TException : Exception
{
    try { await task.WaitAsync(TimeSpan.FromSeconds(10)); }
    catch (TException) { return; }
    throw new InvalidOperationException($"operation succeeded but {typeof(TException).Name} was expected");
}

static void Check(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}
