using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using static GuiWire;

// Host timestamps share one clock. The worker supplies elapsed intervals only.
internal sealed class GuiPhaseTiming
{
    private long launch, spawnReturn, ready, terminal, close, handlerTicks;
    private int requests;
    private sealed record WorkerTimes(long compile_ns, long vm_local_ns, long exchange_ns, int exchange_count);
    private WorkerTimes? worker;

    internal static long Nanoseconds(long ticks) => checked((long)((Int128)ticks * 1_000_000_000 / Stopwatch.Frequency));
    internal void OnLaunch() => launch = Stopwatch.GetTimestamp();
    internal void OnSpawnReturn() => spawnReturn = Stopwatch.GetTimestamp();
    internal void OnReady() => ready = Stopwatch.GetTimestamp();
    internal void OnTerminal() => terminal = Stopwatch.GetTimestamp();
    internal void OnClose() => close = Stopwatch.GetTimestamp();

    internal void OnDispatch(long start)
    {
        handlerTicks += Stopwatch.GetTimestamp() - start;
        requests++;
    }

    internal void AdmitWorker(JsonElement value)
    {
        Fields(value, "compile_ns", "vm_local_ns", "exchange_ns", "exchange_count");
        long Duration(string field)
        {
            var property = value.GetProperty(field);
            if (property.ValueKind != JsonValueKind.Number || !property.TryGetInt64(out long ns)
                || ns < 0 || property.GetRawText() != ns.ToString(CultureInfo.InvariantCulture))
                throw new InvalidDataException("Invalid GUI duration " + field);
            return ns;
        }
        int count = Number(value, "exchange_count");
        if (count != requests) throw new InvalidDataException("GUI worker exchange count differs from host requests");
        worker = new(Duration("compile_ns"), Duration("vm_local_ns"), Duration("exchange_ns"), count);
    }

    internal object Observation() => new
    {
        wall_ns = Nanoseconds(close - launch), startup_ns = Nanoseconds(ready - launch),
        spawn_call_ns = Nanoseconds(spawnReturn - launch),
        shutdown_ns = Nanoseconds(close - terminal), handler_ns = Nanoseconds(handlerTicks),
        request_count = requests, worker_profile = worker
    };
}
