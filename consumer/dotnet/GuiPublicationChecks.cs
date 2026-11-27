using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using static GuiWire;

internal static class GuiPublicationChecks
{
    private sealed record Observations(string draft, string published, int clicks);
    private sealed record Primary(string Outcome, bool? Result = null, int? Effects = null, Observations? After = null);
    private static readonly JsonSerializerOptions Faults = new()
    {
        Converters = { new JsonStringEnumConverter<GuiFault>(JsonNamingPolicy.SnakeCaseLower, allowIntegerValues: false) }
    };

    private static long Nanoseconds(long start, long end) =>
        checked((long)((Int128)(end - start) * 1_000_000_000 / Stopwatch.Frequency));

    internal static async Task RunAsync(string worker, string fixturePath, bool benchmark = false)
    {
        using var document = Parse(await File.ReadAllBytesAsync(fixturePath));
        var fixture = document.RootElement;
        if (Number(fixture, "version") != 1) throw new InvalidDataException("Unknown publication fixture version");
        var directory = Path.GetDirectoryName(Path.GetFullPath(fixturePath))!;
        Task<string> Load(string key) => File.ReadAllTextAsync(Path.Combine(directory, Text(fixture, key)), Utf8);
        var program = await Load("program");
        var readDraft = await Load("read_draft");
        var readPublication = await Load("read_publication");
        var inputs = fixture.GetProperty("arguments");
        string wanted = Text(inputs, "text");
        var ids = new HashSet<string>(StringComparer.Ordinal);
        foreach (var sample in fixture.GetProperty("cases").EnumerateArray())
        {
            string id = Text(sample, "id");
            if (!ids.Add(id)) throw new InvalidDataException($"Duplicate GUI case {id}");
            var fault = sample.GetProperty("fault").Deserialize<GuiFault>(Faults);
            using var binding = new CancellationTokenSource();
            var app = new GuiArticle(fault, Text(fixture, "superseding_text"), binding.Cancel);
            long? firstRequest = null;
            long lastRequest = 0;
            GuiWorker.Reply TimedDispatch(string operation, JsonElement arguments)
            {
                lastRequest = Stopwatch.GetTimestamp();
                firstRequest ??= lastRequest;
                return app.Dispatch(operation, arguments);
            }
            Func<string, JsonElement, GuiWorker.Reply> dispatch = benchmark ? TimedDispatch : app.Dispatch;
            long primaryStart = benchmark ? Stopwatch.GetTimestamp() : 0;
            JsonElement? terminal;
            try { terminal = await GuiWorker.RunAsync(worker, program, inputs, dispatch, binding.Token); }
            catch (OperationCanceledException error) when (binding.IsCancellationRequested && error.CancellationToken == binding.Token)
            {
                terminal = null;
            }
            long primaryEnd = benchmark ? Stopwatch.GetTimestamp() : 0;
            var (outcome, result, effects, after) = ParsePrimary(terminal);

            var commits = new { edits = app.Edits, clicks = app.Clicks };
            async Task<string> Read(string source)
            {
                var empty = JsonSerializer.SerializeToElement(new { });
                var read = await GuiWorker.RunAsync(worker, source, empty, app.Read);
                CheckReportIdentity(read);
                var value = TypedResult(read, "string");
                if (Number(read, "effects") != 2 || app.Edits != commits.edits || app.Clicks != commits.clicks)
                    throw new InvalidDataException($"Readback changed the article or used extra Effects for {id}");
                return value.GetString()!;
            }
            long readbackStart = benchmark ? Stopwatch.GetTimestamp() : 0;
            var readback = new { draft = await Read(readDraft), published = await Read(readPublication) };
            long readbackEnd = benchmark ? Stopwatch.GetTimestamp() : 0;
            if (readback.draft != app.Draft || readback.published != app.Published)
                throw new InvalidDataException($"Readback differs from the application's stored article for {id}");
            bool verified = result == true && after is not null && after.draft == wanted && after.published == wanted
                && after.clicks == 1 && readback.draft == wanted && readback.published == wanted
                && commits.edits == 1 && commits.clicks == 1;
            var observed = JsonSerializer.SerializeToElement(new { outcome, result, effects, verified, report_after = after,
                readback, commits, trace = app.Trace });
            if (!JsonElement.DeepEquals(observed, sample.GetProperty("expected")))
                throw new InvalidDataException($"GUI {id} differs from the shared fixture: {observed.GetRawText()}");
            Console.WriteLine("LESE_GUI " + JsonSerializer.Serialize(new { id, observed }));
            if (benchmark)
            {
                var timing = new { id, primary_ns = Nanoseconds(primaryStart, primaryEnd),
                    request_span_ns = Nanoseconds(firstRequest ?? throw new InvalidDataException("Missing primary request"), lastRequest),
                    readback_ns = Nanoseconds(readbackStart, readbackEnd) };
                Console.WriteLine("LESE_GUI_TIMING " + JsonSerializer.Serialize(timing));
            }
        }
        if (ids.Count == 0) throw new InvalidDataException("Empty GUI fixture");
    }

    private static Primary ParsePrimary(JsonElement? terminal)
    {
        if (terminal is not JsonElement report) return new("cancelled");
        CheckReportIdentity(report);
        if (report.TryGetProperty("status", out var status))
        {
            Fields(report, "schema_version", "kind", "profile", "status", "execution_failure", "error", "possible_partial_effects");
            if (status.GetString() != "failed" || !report.GetProperty("possible_partial_effects").GetBoolean()
                || Text(report, "error").Length == 0) throw new InvalidDataException("Invalid failure report");
            return new(Text(report.GetProperty("execution_failure"), "code"));
        }
        var result = TypedResult(report, "boolean").GetBoolean();
        var controls = report.GetProperty("objects_after").EnumerateArray().ToDictionary(o => Text(o, "identifier"));
        if (controls.Count != 2) throw new InvalidDataException("Invalid article inventory in final report");
        var after = new Observations(Text(controls["draft"], "value"), Text(controls["publish"], "value"),
            Number(controls["publish"], "clicks"));
        return new("reported", result, Number(report, "effects"), after);
    }

    private static void CheckReportIdentity(JsonElement report)
    {
        if (Number(report, "schema_version") != 1 || Text(report, "kind") != "javascript_task_run"
            || Text(report, "profile") != "javascript.registered") throw new InvalidDataException("Unrecognized GUI report");
    }

    private static JsonElement TypedResult(JsonElement report, string type)
    {
        Fields(report, "schema_version", "kind", "profile", "result", "effects", "fuel_remaining", "objects_before", "objects_after");
        var result = report.GetProperty("result");
        Fields(result, "type", "value");
        if (Text(result, "type") != type) throw new InvalidDataException("Unexpected GUI result type");
        return result.GetProperty("value");
    }
}
