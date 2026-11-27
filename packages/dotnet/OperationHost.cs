using System.Diagnostics;
using System.Text.Json;
using static LeseLang.Operations.WorkerWire;

namespace LeseLang.Operations;

/// <summary>Evidence from a cleanly completed VM. The application verifies its intended state change.</summary>
public sealed record OperationReport(string ResultType, JsonElement Result, int Effects, ulong FuelRemaining);

/// <summary>Runs one task at a time through a fresh process and an application-owned registry.</summary>
public sealed class OperationHost
{
    private const string Profile = "lese.operations";
    private readonly string executable;
    private readonly OperationRegistry registry;
    private readonly Func<IReadOnlyCollection<string>> grants;
    private readonly string epoch = Guid.NewGuid().ToString("N");
    private int active;

    public OperationHost(string executable, OperationRegistry registry, Func<IReadOnlyCollection<string>> grants)
    {
        if (!Path.IsPathFullyQualified(executable)) throw new ArgumentException("Expected an absolute worker path", nameof(executable));
        this.executable = executable;
        this.registry = registry ?? throw new ArgumentNullException(nameof(registry));
        this.grants = grants ?? throw new ArgumentNullException(nameof(grants));
    }

    public JsonElement Describe() =>
        JsonSerializer.SerializeToElement(new { schema_version = 1, profile = Profile, operations = registry.Describe() });

    public async Task<OperationReport> ExecuteAsync(string source, IReadOnlyDictionary<string, string> arguments,
        CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (Interlocked.CompareExchange(ref active, 1, 0) != 0) throw new InvalidOperationException("An operation task is already active");
        try
        {
            var inputs = CopyInputs(source, arguments);
            var admitted = CurrentGrants();
            var scope = new Scope("operations", epoch, Guid.NewGuid().ToString("N"));
            var start = Encode(new { agent_worker_version = 4, kind = "start", profile = Profile, scope,
                source, arguments = inputs, operations = registry.Describe(), grants = admitted }, 128 * 1024);
            return await RunAsync(scope, start, admitted, cancellationToken).ConfigureAwait(false);
        }
        finally { Volatile.Write(ref active, 0); }
    }

    private static Dictionary<string, string> CopyInputs(string source, IReadOnlyDictionary<string, string> arguments)
    {
        if (source is null || source.Contains('\0') || Utf8.GetByteCount(source) > 16 * 1024)
            throw new ArgumentException("Invalid task source", nameof(source));
        ArgumentNullException.ThrowIfNull(arguments);
        var copy = new Dictionary<string, string>(StringComparer.Ordinal);
        int total = 0;
        foreach (var (name, value) in arguments)
        {
            if (copy.Count >= 8 || !OperationRegistry.Identifier(name) || value is null || value.Contains('\0'))
                throw new ArgumentException("Invalid task argument", nameof(arguments));
            int bytes = Utf8.GetByteCount(value);
            total += bytes;
            if (bytes > 4096 || total > 16 * 1024) throw new ArgumentException("Task arguments exceed the byte limit", nameof(arguments));
            copy.Add(name, value);
        }
        return copy;
    }

    private HashSet<string> CurrentGrants()
    {
        var current = new HashSet<string>(StringComparer.Ordinal);
        foreach (var cap in grants())
            if (!registry.Capabilities.Contains(cap) || !current.Add(cap)) throw new InvalidOperationException("Invalid application grants");
        return current;
    }

    private sealed record Scope(string window, string epoch, string session);

    private async Task<OperationReport> RunAsync(Scope scope, byte[] start, HashSet<string> admitted, CancellationToken caller)
    {
        using var lifetime = CancellationTokenSource.CreateLinkedTokenSource(caller);
        lifetime.CancelAfter(TimeSpan.FromSeconds(15));
        var token = lifetime.Token;
        using var child = new Process { StartInfo = new ProcessStartInfo(executable)
        {
            ArgumentList = { "--serve" }, UseShellExecute = false, RedirectStandardInput = true,
            RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true
        } };
        token.ThrowIfCancellationRequested();
        if (!child.Start()) throw new IOException("Worker did not start");
        using var registration = token.Register(() => Kill(child));
        var stderr = DrainErrorAsync(child.StandardError.BaseStream, lifetime);
        try
        {
            Stream stdin = child.StandardInput.BaseStream;
            await SendAsync(stdin, start, token).ConfigureAwait(false);
            var reader = new WorkerWire(child.StandardOutput.BaseStream);
            var report = await ExchangeAsync(reader, stdin, scope, admitted, token).ConfigureAwait(false);
            // Keep stdin open until the worker exits. Premature input EOF retires its live session.
            await child.WaitForExitAsync(token).ConfigureAwait(false);
            await stderr.WaitAsync(token).ConfigureAwait(false);
            token.ThrowIfCancellationRequested();
            if (child.ExitCode != 0) throw new InvalidDataException("Worker returned a nonzero exit code");
            return report;
        }
        catch (Exception) when (caller.IsCancellationRequested) { throw new OperationCanceledException(caller); }
        catch (Exception) when (lifetime.IsCancellationRequested)
        {
            if (stderr.IsFaulted) await stderr.ConfigureAwait(false);
            throw new TimeoutException("Operation worker deadline exceeded");
        }
        finally
        {
            // Also fence abandoned handlers before releasing this host for another run.
            lifetime.Cancel();
            await child.WaitForExitAsync().ConfigureAwait(false);
            try { await stderr.ConfigureAwait(false); } catch (OperationCanceledException) { }
        }
    }

    private static void Kill(Process child)
    {
        try { if (!child.HasExited) child.Kill(entireProcessTree: true); }
        catch (InvalidOperationException) when (child.HasExited) { }
    }

    private static async Task DrainErrorAsync(Stream stderr, CancellationTokenSource lifetime)
    {
        byte[] buffer = new byte[4096];
        int count, total = 0;
        try
        {
            while ((count = await stderr.ReadAsync(buffer, lifetime.Token).ConfigureAwait(false)) != 0)
            {
                total += count;
                if (total > 128 * 1024) throw new InvalidDataException("Worker stderr exceeds the byte limit");
            }
        }
        catch { lifetime.Cancel(); throw; }
    }

    private async Task<OperationReport> ExchangeAsync(WorkerWire reader, Stream stdin, Scope scope,
        HashSet<string> admitted, CancellationToken token)
    {
        int delivered = 0;
        while (true)
        {
            byte[] line = await reader.ReadFrameAsync(token).ConfigureAwait(false);
            using var frame = Parse(line);
            var root = frame.RootElement;
            if (Number(root, "agent_worker_version") != 4) throw new InvalidDataException("Unexpected worker version");
            switch (Text(root, "kind"))
            {
                case "request":
                    Fields(root, "agent_worker_version", "kind", "challenge", "request");
                    if (line.Length > RequestLimit || delivered >= 256) throw new InvalidDataException("Worker request limit exceeded");
                    await DispatchAsync(reader, stdin, root, scope, delivered + 1, admitted, token).ConfigureAwait(false);
                    delivered++;
                    break;
                case "report":
                    Fields(root, "agent_worker_version", "kind", "report_json");
                    var report = ReadReport(Text(root, "report_json"), delivered);
                    if (await reader.PeekAsync(token).ConfigureAwait(false)) throw new InvalidDataException("Output after the terminal report");
                    return report;
                case "error":
                    Fields(root, "agent_worker_version", "kind", "error");
                    string message = Text(root, "error");
                    if (message.Length == 0 || Utf8.GetByteCount(message) > 1024) throw new InvalidDataException("Invalid worker error");
                    throw new InvalidDataException("Worker failed: " + message);
                default:
                    throw new InvalidDataException("Unknown worker frame");
            }
        }
    }

    private async Task DispatchAsync(WorkerWire reader, Stream stdin, JsonElement frame, Scope scope,
        int serial, HashSet<string> admitted, CancellationToken token)
    {
        var request = frame.GetProperty("request");
        Fields(request, "transport_version", "window", "epoch", "session", "request_id", "budget_ms", "operation", "arguments");
        string challenge = Text(frame, "challenge");
        int budget = Number(request, "budget_ms");
        if (challenge.Length != 64 || !challenge.All(c => char.IsAsciiDigit(c) || c is >= 'a' and <= 'f')
            || budget is < 1 or > 5000 || Number(request, "transport_version") != 1 || Number(request, "request_id") != serial
            || Text(request, "window") != scope.window || Text(request, "epoch") != scope.epoch || Text(request, "session") != scope.session)
            throw new InvalidDataException("Foreign or malformed worker request");
        string name = Text(request, "operation");
        string capability = registry.CapabilityFor(name);
        using var action = CancellationTokenSource.CreateLinkedTokenSource(token);
        action.CancelAfter(budget);
        var actionToken = action.Token;
        var began = Stopwatch.GetTimestamp();
        // An extra stdout byte or EOF while the reply is pending retires this handler.
        // After sending the reply, let this same lookahead supply the next frame.
        var nextOutput = reader.PeekAsync(token);
        bool Allows(string cap) => admitted.Contains(cap) && CurrentGrants().Contains(cap)
            && !actionToken.IsCancellationRequested && !nextOutput.IsCompleted
            && Stopwatch.GetElapsedTime(began).TotalMilliseconds < budget;
        Task<JsonElement>? operation = null;
        try
        {
            if (!Allows(capability)) throw new InvalidOperationException("Operation authority is unavailable");
            operation = registry.DispatchAsync(name, request.GetProperty("arguments").Clone(), Allows, actionToken);
            var ready = await Task.WhenAny(operation, nextOutput).WaitAsync(actionToken).ConfigureAwait(false);
            if (ready == nextOutput) throw new InvalidDataException("Worker wrote output or closed before the reply");
            var result = await operation.ConfigureAwait(false);
            byte[] reply = Encode(new { agent_worker_version = 4, kind = "reply", challenge,
                reply = new { transport_version = 1, scope.window, scope.epoch, scope.session,
                    request_id = serial, status = "ok", value = result, error = (string?)null } }, RequestLimit);
            if (!Allows(capability)) throw new InvalidOperationException("Operation authority was lost");
            await SendAsync(stdin, reply, actionToken).ConfigureAwait(false);
        }
        catch
        {
            // A handler can finish after cancellation, and the read can fail during
            // process teardown. Observe both tasks without delaying retirement.
            if (operation is not null) ObserveRetired(operation);
            ObserveRetired(nextOutput);
            throw;
        }
        finally { action.Cancel(); }
        if (!await nextOutput.ConfigureAwait(false)) throw new EndOfStreamException("Worker closed before its report");
    }

    private static void ObserveRetired(Task task) =>
        _ = task.ContinueWith(static finished => { _ = finished.Exception; }, CancellationToken.None,
            TaskContinuationOptions.OnlyOnFaulted | TaskContinuationOptions.ExecuteSynchronously, TaskScheduler.Default);

    private static OperationReport ReadReport(string raw, int delivered)
    {
        if (Utf8.GetByteCount(raw) > 128 * 1024) throw new InvalidDataException("Oversized operation report");
        using var document = Parse(Utf8.GetBytes(raw));
        var report = document.RootElement;
        Fields(report, "schema_version", "kind", "profile", "result", "effects", "fuel_remaining");
        int fuel = Number(report, "fuel_remaining");
        if (Number(report, "schema_version") != 1 || Text(report, "kind") != "operation_task_run" || Text(report, "profile") != Profile
            || Number(report, "effects") != delivered || fuel > 10_000) throw new InvalidDataException("Invalid operation report");
        var result = report.GetProperty("result");
        Fields(result, "type", "value");
        string type = Text(result, "type");
        var value = result.GetProperty("value");
        OperationRegistry.ValidateWireValue(type, value);
        return new(type, value.Clone(), delivered, (ulong)fuel);
    }
}
