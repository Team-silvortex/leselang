# Registered GUI worker phase timings

This benchmark follows a complete Node or C# Consumer run through its child
workers. Each of the 15 publication cases starts a primary worker and two
read-only workers on the same in-memory application. A sample must pass the
fixture's checks for the terminal outcome, request trace, committed edits and
clicks, and both readback values. [Shared workload](../fixtures/registered-gui/README.md)

## Run the suite

```sh
sh scripts/check.sh benchmark-registered-gui --worker-phases
```

The command builds the Release artifacts before sampling. Use
`--warmup 0 --samples 2` for a smoke run. The defaults are three discarded
warm-up rounds and 30 measured rounds. The runner rotates both the Consumer
order and the starting case each round. Each host gets a fresh Consumer
process per round, and each task gets a fresh worker.

After building, run the following commands on the same machine to compare
suite wall times with profiling disabled and enabled. Use matching sample
counts, binaries, inputs and background load. The ordinary run includes the
embedded Rust Consumer as well. Compare the Node and C# rows for their
respective paths. The difference between the two runs includes run-to-run
variation along with the ready handshake, clock reads and telemetry output.

```sh
node benchmarks/registered-gui.mjs
node benchmarks/registered-gui.mjs --worker-phases
```

## How the time is attributed

In profiling mode the Consumer launches `--profile-gui --serve` and waits for
the worker's `profile_ready` frame before sending the v3 start record. A
completed worker places its elapsed durations in the outer report envelope.
The Consumer's monotonic clock measures the worker lifetime and the local
handler calls. Each subtraction uses either timestamps from the same clock or
elapsed durations for intervals known to nest. [Worker timers](../hosts/registered/src/gui_profile.rs)
· [Attribution code](worker-phases.mjs)

| Component | Measured interval or calculation |
| --- | --- |
| `spawn_call_ns` | Host time spent in the synchronous process launch call. |
| `ready_wait_ns` | From that call returning to the host accepting the ready frame. Together these first two components give `startup_ns`. |
| `compile_ns` | Worker preparation of the Lese program, including source and argument validation, compilation and entry binding. |
| `vm_local_ns` | Worker VM drive time, less the synchronous exchanges nested inside that drive. Includes local Effect processing and VM diagnostics. |
| `handler_ns` | Accumulated host time in the GUI request handlers. The Node measurement includes awaiting the handler result. |
| `protocol_ns` | Summed worker round-trip time (`exchange_ns`) less `handler_ns`. Includes request correlation, framing, host validation outside the timed handler, pipe I/O and scheduling around replies. Covers bootstrap and final verification as well as program Effects. |
| `session_other_ns` | Time between ready and terminal receipt remaining after subtracting compile, VM-local and full exchange durations. Includes start/report framing and worker-side GUI setup and verification outside the measured drive and exchanges. |
| `shutdown_ns` | From the host recognizing the terminal frame or beginning cancellation to observed process closure. Includes host work within those endpoints. |
| `consumer_other_ns` | Suite wall time outside the 45 worker lifetimes, including Consumer startup, fixture handling, verification and emitting results. |

C# parses the inner business report before waiting for EOF and process exit,
so that parsing falls inside its shutdown interval. Node parses it after the
transport observes closure, which puts that work in its Consumer remainder.

For the two cancelled primaries, the host knows when it started, received
ready, cancelled and closed the worker. Killing the child leaves no terminal
worker telemetry. These rows have `worker_profile: null`. The time from ready
to cancellation appears once as `interrupted_active_ns`. The two subsequent
read-only sessions provide complete observations and timings. Tasks that
return a typed execution failure can still deliver a terminal worker profile.
The cancellation path uses `Process.Kill(entireProcessTree: true)` in C# and
signals the child via `child.kill` in Node.

## Read the output

The runner prints a fresh `target/benchmarks/worker-phases.*` directory.
`samples.ndjson` contains the launch-to-close lifetime, host counters and
worker profile for each case and session (`primary`, `read_draft` and
`read_publication`). The driver pairs the worker exchange count with the host
request count. It checks the expected trace length for the primary and nine
requests in each readback, including discovery and final observations.

In `summary.json`, `results.<consumer>.attribution` partitions the measured
suite wall time. Each percentage divides the component's accumulated duration
by accumulated suite time over all measured rounds. Those shares total 100%.
The per-component p50 and p95 describe separate distributions. The per-case
session summaries also include `startup_ns` and, when available,
`exchange_ns` as inclusive intervals. Each
`completed_session_exchanges` mean weights every exchange equally across all
sessions with terminal telemetry, including typed failures. Its RTT includes
host handlers. Its protocol-only mean removes their measured duration. Use
the per-case and per-session distributions to distinguish publication from
the two fresh reads. [Base benchmark and percentile convention](README.md)

## Apple M2 baseline, 2026-11-27

The same Release builds ran three times on macOS 27.0.1 arm64 with Node
24.6.0 and .NET 10.0.5. Each run discarded three warm-ups and measured 30
rounds. In each profiled run, each Consumer started 1,350 workers. Of these,
1,290 delivered a terminal profile covering 12,690 protocol exchanges, and
60 were cancelled as specified by the fixture. The build used the working
tree at `a05960e` with the profiling changes applied.
[Environment, artifact digests and per-case results](baselines/2026-11-27-macos-arm64/worker-phases/summary.json)

| 15-case suite, p50 (ms) | Node | C# |
| --- | ---: | ---: |
| [First profile](baselines/2026-11-27-macos-arm64/worker-phases/samples.ndjson.gz) | 209.00 | 938.47 |
| [Ordinary control](baselines/2026-11-27-macos-arm64/worker-phases/control.samples.ndjson.gz) | 206.69 | 924.90 |
| [Repeat profile](baselines/2026-11-27-macos-arm64/worker-phases/repeat.samples.ndjson.gz) | 205.33 | 802.26 |

| Share of total suite time in the first profile | Node | C# |
| --- | ---: | ---: |
| Synchronous `spawn()` / `Process.Start()` call | 7.7% | 64.8% |
| Launch call returned to ready frame accepted | 33.2% | 9.7% |
| Terminal or cancellation to observed exit | 6.8% | 6.2% |
| Protocol round trips, excluding measured host handlers | 16.8% | 7.1% |
| Host handlers in completed sessions | 3.5% | 0.2% |
| Lese preparation and compilation | 2.1% | 0.6% |
| VM drive, excluding round trips | 1.4% | 0.4% |
| Other work within completed worker sessions | 2.8% | 2.8% |
| Active interval of cancelled workers | 1.4% | 0.6% |
| Consumer time outside all worker lifetimes | 24.2% | 7.7% |

The synchronous process launch averaged 0.37 ms per worker in Node and
12.93 ms in C#. With 45 launches per round, this interval accounted for
about 83% of the difference in mean suite time. On repeat, the same call
accounted for 7.7% of Node time and 63.5% of C# time. The C# suite median
fell 14.5% between profiles. The ordinary control lay between those runs
for both hosts, so these measurements do not isolate a fixed profiling cost.

In the successful `publish` primary, 9 program Effects required 16 protocol
round trips including discovery and verification. The median cumulative
RTT was 1.56 ms for Node and 2.50 ms for C#. Across all exchanges with
terminal telemetry, a single round trip averaged 104 µs and 154 µs
respectively, including the host handler. Median compilation on `publish`
took 0.247 ms and 0.265 ms. Local VM drive took 0.194 ms and 0.232 ms.
Successful publication closed in a median 0.30 ms and 0.23 ms after the
terminal frame. For the two explicit cancellations, the corresponding
shutdown medians were 0.34–0.38 ms in Node and 16.26–17.24 ms in C#.
