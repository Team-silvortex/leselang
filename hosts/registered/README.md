# Standalone Registered-Object Host

`leselang-registered-host` owns the portable Rust execution behind registered
JavaScript application objects. It is a separate Cargo workspace with a library
and the `leselang-agent-worker` binary. Its only direct dependencies are the
LeseLang VM, serde and serde_json. No Tauri, WebView, Node or GUI framework is
needed to build or install the worker.

The application supplies live objects and verifies their actual state. The
library owns typed compilation, bounded task admission, native identities,
permissions, Fuel and explicit `poll → authorize → dispatch → accept`. It does
not evaluate JavaScript, select application tasks, retry effects or roll back
committed actions.

## Build And Use

From the repository root, with the pinned Rust dependencies available:

```sh
cargo build --manifest-path hosts/registered/Cargo.toml --locked --offline --profile lean --bin leselang-agent-worker
target/lean/leselang-agent-worker --describe
sh scripts/check.sh registered
```

The `--describe` command emits the `javascript.registered` base catalog without
starting an execution. `--serve` admits exactly one task over bounded stdio and
returns its original typed report. Use the public `@leselang/bridge/node`
transport to drive that protocol from JavaScript. See the
[Node host guide](../../docs/node-agent-host.md) for composition, installation,
wire limits, cancellation and partial-effect semantics.

Rust applications may depend on this library directly and implement `Transport`.
`prepare_task` compiles and validates zero or one named string input before any
transport request. The opt-in `prepare_named_task` accepts zero to eight named
strings, each at most 4096 UTF-8 bytes and at most 16 KiB in total. Both entry
points compile the same language and bind every declared parameter before any
application request. `run_prepared_registered_task` consumes either prepared task
and emits `javascript_task_run` evidence. Its report has no business-success
claim; the application must verify its own declared conditions and fresh state.
The independent [Rust Consumer](tests/consumer/src/main.rs) runs two Effects
using only these public APIs.

The additive `run_prepared_registered_task_detailed` entry returns
`ExecutionFailure`, with a closed `ExecutionFailureCode` and the original error
text. The old string-error entry projects the same implementation; there is still
one VM driver. Classification uses typed host errors and never interprets an
arbitrary transport message as a native cause. The public `Error` enum and
legacy Node worker error frames are unchanged. Tauri and explicitly opted-in
Node workers carry these details in the
[execution failure report contract](../../docs/execution-reports.md).

Semantic v3 plans explicitly carry `inputMode: "named_strings"`, including
zero-input tasks. The Node transport maps this marker to worker protocol v2;
startup, requests, replies and terminal records all carry
`agent_worker_version: 2`. The source and named-argument map retain their existing
shape, while the inner Bridge protocol and execution-report schema remain v1.
The marker's absence retains worker v1 and its zero-or-one-string contract.
Unsupported versions fail without fallback, and the worker pins each reply to
its selected version, scope, serial and challenge. See the
[named string input guide](../../docs/named-task-inputs.md) for declarations,
binding rules, bounds and failures.

Transport `executionReports: "v1"` explicitly selects worker protocol v3. Its
start record requires `input_mode: "single_string"` or `"named_strings"`, retaining
the chosen input limit; v1/v2 reject this field. An admitted task that fails while
the transport remains live can produce a bounded failed report from the detailed
entry. Successful reports remain unchanged. Admission, broken transport and
output errors retain the original error channel; cancellation does not fabricate
a report. Node still requires clean process closure before delivering either a
successful or failed report to the host's verifier.

For application-declared scalar operations, `createNodeOperationHost` selects
worker v4. Its start frame carries operation signatures and the host's grant
snapshot. The worker registers them with the Rust embedding API, compiles the
entry, and sends each authorized Effect to the application. The success report
echoes the selected profile. New host languages can select `lese.operations`;
Node also supports its historical `javascript.operations` identity. The
application owns discovery and verification. See the
[scalar application host contract](../../docs/operations-host-protocol.md).

## Ownership And Compatibility

| Source | Responsibility |
| --- | --- |
| `src/lib.rs` | Public types, offline catalog projection and explicit VM driver |
| `src/failure.rs` | Optional typed execution failures and legacy error-text preservation |
| `src/catalog.rs` | Shared operation/type metadata used by compilation and discovery |
| `src/gateway.rs` | Bounded registered-object transport, observations and lifecycle checks |
| `src/host.rs` | Local object authority, metering, dispatch and reply acceptance |
| `src/task.rs` | Prepared scalar task admission and fixed registered report identity |
| `src/worker.rs`, `src/worker/json_lines.rs` | One-task stdio process and bounded JSON Lines framing |
| `src/worker/operations.rs` | Worker v4 declaration admission, scalar encoding and explicit VM driving |
| `src/bin/leselang-agent-worker.rs` | Standalone CLI entry |
| `tests/boundary.rs`, `tests/consumer/` | Resolved dependency isolation and independent Rust usage |

The Tauri library re-exports these same Rust types so existing `Scope`,
`Transport`, `Error`, `PreparedTask`, host and gateway values remain compatible.
Tauri keeps its application catalogs, form constants, desktop IPC and fixed
report naming in its own workspace. Its old `agent_worker` example delegates to
this worker; the old Rust-launched `node_host` retains its behavior and privately
includes the canonical framing helper. Both use the registered-object VM driver and task compiler.
The Tauri frontend selects `agent_start_named_task` only for the explicit named
input marker; its original command and v2 form and board catalogs keep their
existing behavior. Worker `--describe` still emits the unchanged base catalog.

The historical three-control gateway constructor remains available for old Rust
callers. New application inventories use `RemoteGui::registered`. These host
APIs do not alter the lower-level language `OperationCatalog` or `LocalVm` APIs.

This package is not published to crates.io, and no prebuilt binary is bundled
in the npm SDK. Local installation uses repository-local language dependencies.
Worker termination does not undo an already completed JavaScript effect; see
the Node guide for the difference between process closure and immediate Bridge
revocation on explicit session cancellation.
