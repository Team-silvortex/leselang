using System.Text;
using System.Text.Json;
using LeseLang.Operations;

// The Consumer apphost doubles as a deliberately misbehaving peer for these
// two checks. All domain examples use the real Rust compiler and VM.
internal static class ProtocolChecks
{
    public static async Task RunAsync()
    {
        var worker = Path.Combine(AppContext.BaseDirectory, OperatingSystem.IsWindows() ? "LeseLang.Consumer.exe" : "LeseLang.Consumer");
        var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var fenced = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        int calls = 0, commits = 0;
        var directory = Directory.CreateTempSubdirectory("lese-dotnet-peer-");
        string marker = Path.Combine(directory.FullName, "handler 已进入");
        try
        {
            var registry = new OperationRegistry([new OperationDefinition("peer.touch", new Dictionary<string, string>(),
                "none", "peer.write", async (_, context) =>
                {
                    Interlocked.Increment(ref calls);
                    File.WriteAllText(marker, "");
                    await release.Task;
                    try { context.Commit(() => Interlocked.Increment(ref commits)); }
                    finally { fenced.TrySetResult(); }
                    return null;
                })]);
            var host = new OperationHost(worker, registry, () => ["peer.write"]);
            var arguments = new Dictionary<string, string> { ["ready"] = marker };
            try { await ExpectFailure(host.ExecuteAsync("pending", arguments)); }
            finally { release.TrySetResult(); }
            await fenced.Task.WaitAsync(TimeSpan.FromSeconds(5));
            if (calls != 1 || commits != 0) throw new Exception("Early worker output reached a second or late handler");
            Console.WriteLine("Passed out-of-order worker request: no second invocation or late commit.");
            var clean = await host.ExecuteAsync("clean", arguments);
            if (clean.ResultType != "none" || clean.Effects != 0) throw new Exception("Wrong control report");
            await ExpectFailure(host.ExecuteAsync("nonzero", arguments));
            Console.WriteLine("Passed exit-status check: identical report accepted on clean exit and rejected on nonzero exit.");
        }
        finally { directory.Delete(true); }
    }

    private static async Task ExpectFailure(Task<OperationReport> pending)
    {
        try { await pending; }
        catch (InvalidDataException) { return; }
        throw new Exception("An invalid worker exchange delivered a report");
    }

    public static async Task<int> ServeAsync()
    {
        // The parent sends UTF-8 regardless of this machine's console code page.
        var utf8 = new UTF8Encoding(false, true);
        using var input = new StreamReader(Console.OpenStandardInput(), utf8, detectEncodingFromByteOrderMarks: false);
        using var output = new StreamWriter(Console.OpenStandardOutput(), utf8) { NewLine = "\n", AutoFlush = true };
        using var start = JsonDocument.Parse(await input.ReadLineAsync() ?? throw new EndOfStreamException());
        var root = start.RootElement;
        string? source = root.GetProperty("source").GetString();
        Task Emit(object frame) => output.WriteLineAsync(JsonSerializer.Serialize(frame));
        if (source == "pending")
        {
            var scope = root.GetProperty("scope");
            async Task EmitRequest(int serial)
            {
                var request = new { transport_version = 1, window = scope.GetProperty("window"), epoch = scope.GetProperty("epoch"),
                    session = scope.GetProperty("session"), request_id = serial, budget_ms = 5000,
                    operation = "peer.touch", arguments = new Dictionary<string, string>() };
                await Emit(new { agent_worker_version = 4, kind = "request", challenge = new string('a', 64), request });
            }
            await EmitRequest(1);
            string ready = root.GetProperty("arguments").GetProperty("ready").GetString()!;
            while (!File.Exists(ready)) await Task.Delay(5);
            await EmitRequest(2);
            await input.ReadLineAsync();
            return 0;
        }
        var report = new { schema_version = 1, kind = "operation_task_run", profile = "lese.operations",
            result = new { type = "none", value = (string?)null }, effects = 0, fuel_remaining = 9_999 };
        await Emit(new { agent_worker_version = 4, kind = "report", report_json = JsonSerializer.Serialize(report) });
        return source == "nonzero" ? 9 : 0;
    }
}
