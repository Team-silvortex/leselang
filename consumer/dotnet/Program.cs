using System.Globalization;
using System.Runtime.InteropServices;
using System.Text.Json;
using LeseLang.Operations;

if (args is ["--serve"]) return await ProtocolChecks.ServeAsync();
if (args is [var guiMode, var guiWorker, var publicationFixture]
    && guiMode is "--registered-gui" or "--benchmark-registered-gui" or "--profile-registered-gui")
{
    try
    {
        var measurement = guiMode switch
        {
            "--profile-registered-gui" => GuiMeasurement.Phases,
            "--benchmark-registered-gui" => GuiMeasurement.Timings,
            _ => GuiMeasurement.None
        };
        await GuiPublicationChecks.RunAsync(guiWorker, publicationFixture, measurement);
        return 0;
    }
    catch (InvalidDataException error)
    {
        Console.Error.WriteLine(error.Message);
        return 1;
    }
}
if (args.Length is not (2 or 3))
{
    Console.Error.WriteLine("Usage: LeseLang.Consumer <absolute worker path> <two-effects.json> [absolute lost-reply relay path]");
    Console.Error.WriteLine("       LeseLang.Consumer (--registered-gui | --benchmark-registered-gui | --profile-registered-gui) <absolute worker path> <publication.json>");
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
    && meter.AddCalls == 1 && meter.CommittedWrites == 1 && meter.ReadCalls == 1, "the application did not observe the requested increment");
Console.WriteLine($"Verified two Effects. Balance {meter.Balance}, Fuel remaining {report.FuelRemaining}.");

await CheckSharedFailureVectors();
if (args.Length == 3) await LostReplyChecks.RunAsync(executable, args[2]);
await CheckAwaitedWrite(cancel: false);
Console.WriteLine("Passed revocation during await: pending write and stale context fenced, subsequent run verified.");
await CheckAwaitedWrite(cancel: true);
Console.WriteLine("Passed cancellation during await: pending write and stale context fenced, subsequent run verified.");
await CheckSwallowedDenial();
Console.WriteLine("Passed swallowed authorization denial: context stayed retired after regrant.");
await CheckScalarRoundTrips();
Console.WriteLine("Passed six scalar types, including exact u64 maximum and optional-string absence.");
await CheckFilterSelection();
Console.WriteLine("Passed exact list membership and conditional filter selection.");
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

async Task CheckSharedFailureVectors()
{
    using var stream = typeof(Meter).Assembly.GetManifestResourceStream("failure-vectors.json")
        ?? throw new InvalidOperationException("Missing failure vectors");
    using var document = await JsonDocument.ParseAsync(stream);
    var vectors = document.RootElement;
    Check(vectors.GetProperty("entry").GetString() == "main", "worker requires the main entry");
    var program = vectors.GetProperty("source").GetString()!;
    var arguments = new Dictionary<string, string>();
    var initialBalance = ulong.Parse(vectors.GetProperty("initial_balance").GetString()!, NumberStyles.None, CultureInfo.InvariantCulture);
    var remaining = new HashSet<string>(["missing-grant", "post-write-invalid-result"]);
    foreach (var sample in vectors.GetProperty("cases").EnumerateArray())
    {
        var id = sample.GetProperty("id").GetString()!;
        Check(remaining.Remove(id), $"Unexpected or repeated failure vector {id}");
        var grants = sample.GetProperty("grants").EnumerateArray().Select(g => g.GetString()!).ToArray();
        var app = new Meter(initialBalance)
        {
            BadWriteResult = sample.GetProperty("write_reply").GetString() switch
            {
                "balance" => false,
                "string" => true,
                var mode => throw new InvalidOperationException($"Unknown write reply {mode}")
            }
        };
        var runner = new OperationHost(executable, app.Registry, () => grants);
        await MustFail<InvalidDataException>(runner.ExecuteAsync(program, arguments));
        var observed = JsonSerializer.SerializeToElement(new
        {
            outcome = "failed",
            add_calls = app.AddCalls,
            read_calls = app.ReadCalls,
            committed_writes = app.CommittedWrites,
            balance = app.Balance.ToString(CultureInfo.InvariantCulture)
        });
        Check(JsonElement.DeepEquals(observed, sample.GetProperty("expected")), $"{id} observed {observed}");

        // The same program and host must complete once full grants and a valid reply are restored.
        grants = app.Grants().ToArray();
        app.BadWriteResult = false;
        await CheckFreshRun(runner, app, program, arguments);
        Console.WriteLine($"Shared failure {id}: {observed.GetRawText()}");
    }
    Check(remaining.Count == 0, $"Missing failure vectors: {string.Join(", ", remaining)}");
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
    Check(app.Balance == Meter.Initial && app.AddCalls == 1 && app.CommittedWrites == 0 && app.ReadCalls == 0, "the stopped handler committed");

    app.CanWrite = true;
    app.BeforeCommit = null;
    await CheckFreshRun(runner, app, source, inputs);
    var lateWrites = 0;
    await MustFail<InvalidOperationException>(Task.Run(() => stale.Commit(() => lateWrites++)));
    Check(lateWrites == 0, "old context acquired authority in a later run");
}

static async Task CheckFreshRun(OperationHost runner, Meter app, string program, IReadOnlyDictionary<string, string> arguments)
{
    var startingBalance = app.Balance;
    var addCalls = app.AddCalls;
    var readCalls = app.ReadCalls;
    var writes = app.CommittedWrites;
    var next = await runner.ExecuteAsync(program, arguments);
    Check(next.Effects == 2 && next.ResultType == "integer"
        && next.Result.GetString() == (startingBalance + 1).ToString(CultureInfo.InvariantCulture)
        && app.Balance == startingBalance + 1 && app.AddCalls == addCalls + 1 && app.ReadCalls == readCalls + 1
        && app.CommittedWrites == writes + 1,
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

async Task CheckFilterSelection()
{
    using var stream = typeof(Meter).Assembly.GetManifestResourceStream("list-membership.json")
        ?? throw new InvalidOperationException("Missing filter fixture");
    using var fixture = await JsonDocument.ParseAsync(stream);
    var source = fixture.RootElement.GetProperty("source").GetString()!;
    foreach (var sample in fixture.RootElement.GetProperty("cases").EnumerateArray())
    {
        var options = sample.GetProperty("options").EnumerateArray().Select(value => value.GetString()!).ToArray();
        var wanted = sample.GetProperty("wanted").GetString()!;
        var matched = sample.GetProperty("matched").GetBoolean();
        var reads = 0;
        var selections = new List<string>();
        var registry = new OperationRegistry([
            new("filters.available", new Dictionary<string, string>(), "string_list", "filters.read", (_, _) =>
            {
                reads++;
                return ValueTask.FromResult<object?>(options);
            }),
            new("filters.select", new Dictionary<string, string> { ["value"] = "string" }, "boolean", "filters.write", (args, context) =>
            {
                context.Commit(() => selections.Add((string)args["value"]!));
                return ValueTask.FromResult<object?>(true);
            }),
        ]);
        var runner = new OperationHost(executable, registry, () => ["filters.read", "filters.write"]);
        var report = await runner.ExecuteAsync(source, new Dictionary<string, string> { ["wanted"] = wanted });
        Check(report.ResultType == "boolean" && report.Result.GetBoolean() == matched
            && report.Effects == (matched ? 2 : 1) && reads == 1
            && (matched ? selections.SequenceEqual([wanted]) : selections.Count == 0),
            "unexpected filter selection");
    }
}

static void Check(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}
