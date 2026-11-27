using System.Diagnostics;
using System.Text.Json;
using static GuiWire;

// Test peer for registered GUI tasks. The public scalar OperationHost uses worker v4.
internal static class GuiWorker
{
    internal sealed record Reply(object? Value, string? Error = null);

    internal static async Task<JsonElement> RunAsync(string executable, string source, JsonElement arguments,
        Func<string, JsonElement, Reply> dispatch, CancellationToken binding = default)
    {
        if (!Path.IsPathFullyQualified(executable)) throw new ArgumentException("Expected an absolute worker path");
        var scope = new { window = "editor", epoch = "opened", session = Guid.NewGuid().ToString("N") };
        using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(15));
        var token = deadline.Token;
        using var child = Process.Start(new ProcessStartInfo(executable)
        {
            ArgumentList = { "--serve" }, UseShellExecute = false, RedirectStandardInput = true,
            RedirectStandardOutput = true, CreateNoWindow = true
        }) ?? throw new IOException("GUI worker did not start");
        try
        {
            var input = child.StandardInput.BaseStream;
            using var output = new GuiWire(child.StandardOutput.BaseStream);
            await WriteFrameAsync(input, new { agent_worker_version = 3, kind = "start", input_mode = "single_string",
                scope, source, arguments }, 128 * 1024, token);
            for (int serial = 1; ; serial++)
            {
                var bytes = await output.ReadFrameAsync(token);
                using var document = Parse(bytes);
                var root = document.RootElement;
                if (Number(root, "agent_worker_version") != 3) throw new InvalidDataException("Unexpected GUI worker version");
                switch (Text(root, "kind"))
                {
                    case "report":
                        Fields(root, "agent_worker_version", "kind", "report_json");
                        var raw = Utf8.GetBytes(Text(root, "report_json"));
                        if (raw.Length > 128 * 1024) throw new InvalidDataException("Oversized GUI report");
                        using (var report = Parse(raw))
                        {
                            await output.RequireEndAsync(token);
                            await child.WaitForExitAsync(token);
                            if (child.ExitCode != 0) throw new InvalidDataException($"GUI worker exit code {child.ExitCode}");
                            return report.RootElement.Clone();
                        }
                    case "error":
                        throw new InvalidDataException("GUI worker protocol error: " + Text(root, "error"));
                    case "request":
                        break;
                    default:
                        throw new InvalidDataException("Unknown GUI worker frame");
                }

                Fields(root, "agent_worker_version", "kind", "challenge", "request");
                var request = root.GetProperty("request");
                Fields(request, "transport_version", "window", "epoch", "session", "request_id", "budget_ms", "operation", "arguments");
                string challenge = Text(root, "challenge");
                if (bytes.Length > RequestLimit || serial > 256 || Number(request, "request_id") != serial
                    || Number(request, "transport_version") != 1 || Number(request, "budget_ms") != 5000
                    || Text(request, "window") != scope.window || Text(request, "epoch") != scope.epoch
                    || Text(request, "session") != scope.session || challenge.Length != 64
                    || !challenge.All(c => char.IsAsciiDigit(c) || c is >= 'a' and <= 'f'))
                    throw new InvalidDataException("Foreign or malformed GUI request");
                var response = dispatch(Text(request, "operation"), request.GetProperty("arguments"));
                if (binding.IsCancellationRequested)
                {
                    // The app retired its binding with a request still outstanding. Do not send an ACK.
                    child.Kill(entireProcessTree: true);
                    await child.WaitForExitAsync(token);
                    await output.RequireEndAsync(token);
                    if (child.ExitCode == 0) throw new InvalidDataException("Cancelled GUI worker completed successfully");
                    throw new OperationCanceledException(binding);
                }
                var reply = new { transport_version = 1, scope.window, scope.epoch, scope.session, request_id = serial,
                    status = response.Error is null ? "ok" : "rejected", value = response.Value, error = response.Error };
                await WriteFrameAsync(input, new { agent_worker_version = 3, kind = "reply", challenge, reply }, RequestLimit, token);
            }
        }
        finally
        {
            try { if (!child.HasExited) child.Kill(entireProcessTree: true); }
            catch (InvalidOperationException) when (child.HasExited) { }
            await child.WaitForExitAsync();
        }
    }
}
