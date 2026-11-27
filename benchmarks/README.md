# LeseLang benchmarks

## Registered GUI publication

This suite measures the time an application spends running and checking a
registered GUI task. Rust embeds the reference VM. Node and C# serve the GUI
protocol from their own processes and start a fresh reference worker for each
task. All three applications use in-memory controls and run the same Lese source.
[Workload and fault definitions](../fixtures/registered-gui/README.md)

To measure worker launch, protocol round trips, GUI handlers, Lese
compilation and VM execution for Node and C#, run this benchmark with
`--worker-phases`. [Worker phase timings](worker-phases.md)

Run the suite from the repository root with the pinned Cargo and npm
dependencies and the .NET 10 SDK installed.

```sh
sh scripts/check.sh benchmark-registered-gui
```

The command builds optimized Rust binaries, the JavaScript SDK and the Release
C# Consumer before starting the clocks. To try the workflow quickly, append
`--warmup 0 --samples 2`. Once the artifacts are built, running
`node benchmarks/registered-gui.mjs` directly repeats the measurements.
[Dependency setup](../docs/testing.md)

### What each duration covers

| Metric | Start and end |
| --- | --- |
| `suite_process_ns` | The driver starts one Consumer process and waits for it to exit. Includes runtime startup, fixture loading, all 15 cases, verification and output. |
| `primary_ns` | The Consumer starts the primary publication task and waits for a terminal result, failure or cancellation. Includes compilation and execution. Node and C# also start, communicate with and close a worker. |
| `request_span_ns` | From entry to the first primary GUI request callback to entry to the last, on the Consumer clock. Includes intervening VM work, request processing and any IPC. The final callback's work falls after the endpoint. |
| `readback_ns` | Two successive read-only tasks, including setup, execution and checking the individual reads. Each gets a fresh session on the same application. Node and C# start two new workers. |

Each round runs the three Consumers sequentially. Each Consumer starts a new
process for the suite and creates a fresh article for each case. The driver
rotates both the host order and the first case by one place each round. The
three warm-up rounds are checked and discarded. Over the 30 measured rounds
each case occupies each of the 15 positions twice, and each host starts ten
rounds. Consequently the results include runtime initialization and normal
diagnostic output in the proportions present in this workflow. File caches
can stay warm across processes. The first invocation in each process can
still incur startup and JIT costs.

A timing counts only after the primary report, request sequence, actual
commits and two independent reads agree with the shared fixture. A rejection
or cancellation is a valid sample when it matches the case. A missing record
or an unexpected outcome fails the run. The suite uses one monotonic clock
per measured interval. The request span is nested inside the primary
duration. Failure cases can reach different numbers of requests, so compare
the same case and metric between runs.

### Results and comparisons

The command prints a new `target/benchmarks/registered-gui.*` directory.
`samples.ndjson` holds one row per measured round and Consumer, in execution
order, with integer nanoseconds for all 15 cases. `summary.json` contains
the median, nearest-rank p95, run configuration, machine and runtime versions,
Git revision and dirty flag, and SHA-256 fingerprints for the inputs, measured
artifacts and raw samples. For an even sample count, the median is the mean
of the middle pair. Runs with fewer than 20 samples leave p95 null.

Use the comparison command with two summaries produced with the same
workload, configuration and environment.

```sh
node benchmarks/compare-registered-gui.mjs BASELINE.json CURRENT.json
```

The comparison prints the p50 values in nanoseconds, their signed difference
and the ratio of current to baseline. Values above 1 mean the new run took
longer. Keep raw samples when investigating a change, and repeat the suite on
the same machine to estimate local variation.

### First baseline, 2026-11-27

Two consecutive runs on an 8-core Apple M2 with 16 GiB of memory used macOS
27.0.1 arm64, Rust 1.98.0, Node 24.6.0 and .NET 10.0.5 (SDK 10.0.201).
Each run measured 30 rounds after three warm-ups. The builds used the local
working tree at `1880bb0` with uncommitted changes. The saved summary includes
the artifact fingerprints for this build.

| Consumer | Publish primary p50 / p95 (ms) | Publish request span p50 (ms) | Two readbacks p50 (ms) | 15-case suite p50 (ms) | Repeat suite p50 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Embedded Rust | 0.312 / 0.482 | 0.172 | 0.137 | 8.62 | 8.75 |
| Node with worker | 4.21 / 9.66 | 1.68 | 6.29 | 203.68 | 205.48 |
| C# with worker | 20.49 / 44.68 | 2.70 | 34.73 | 908.05 | 899.01 |

The whole-suite medians moved by at most 1.5% on the repeat. Individual
cases varied more. For example, the C# publish median was 18.21 ms on the
second run. The publish case verifies one edit and one click after nine
Effects; the two subsequent reads use separate sessions.
[Baseline summary](baselines/2026-11-27-macos-arm64/summary.json)
· [Raw samples](baselines/2026-11-27-macos-arm64/samples.ndjson)
