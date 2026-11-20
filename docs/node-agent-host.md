# Run An Application-Owned Node Agent With A Rust Worker

The independent Node application keeps its own model alive while each task
executes in a fresh Rust worker. It declares its controls with
[`createRegisteredApplication`](registered-agent-host.md#declare-the-application-controls),
uses the public [`createRegisteredAgentHost`](registered-agent-host.md), and adds the optional public
`createNodeAgentTransport` from `@leselang/bridge/node`. The Rust process compiles LeseLang and drives
the existing VM; the JavaScript process handles registered-object requests and
independently verifies actual application state.

This reference application has five objects: `draft`, `publish`, `status`,
`reset` and `unrelated`. Publication copies the current draft into the published
value, increments a counter and updates status. These are in-memory
demonstration values, not a connection to an external publication service or a
persistent database. Worker replacement does not reset the parent application.

## Build And Run

With the pinned Rust and npm dependencies already installed, run from the
repository root:

```sh
npm run build --prefix packages/javascript
cargo build --manifest-path hosts/registered/Cargo.toml --locked --offline --profile lean --bin leselang-agent-worker
npm run agent-demo --prefix consumer/javascript
npm run agent-demo --prefix consumer/javascript -- --text 'Article "文本" ✓'
```

This path builds the standalone worker without resolving or compiling Tauri.
Run `sh scripts/check.sh registered` for its library, protocol and independent
Rust Consumer checks; `sh scripts/check.sh node-consumer` additionally tests
the Node applications and old Tauri compatibility entry. The SDK build and
Consumer npm dependencies are prerequisites. See
[testing setup](testing.md) for the pinned environment.

The demo follows one explicit offline plan:

1. Read the current draft and verify its actual string result.
2. Require that result to match the supplied text, then bind it as the input to
   the publication task.
3. Read the published value and verify it against fresh application state.
4. Check the complete publication inventory without changing it.

The successful plan has 14 VM Effects: `2 + 9 + 2 + 1`. Its stdout record shows
the outcome, per-step results and retained application model. This provider
constructs a declared workflow directly; it does not interpret natural language,
contact a model or consume model credits.

To exercise a failure after the actual publication mutation:

```sh
npm run agent-demo --prefix consumer/javascript -- --text 'Retained publication' --fault-after-commit
```

This command intentionally exits unsuccessfully. Publication remains in the
application, the workflow reports possible partial effects, and later steps do
not run. There is no automatic retry or rollback.

The worker is the `leselang-agent-worker` binary in the independent
[`hosts/registered`](../hosts/registered/README.md) workspace. Its library owns
the registered-object gateway, task admission and explicit VM driver. Its
dependency graph contains no Tauri or WebView framework, including optional
dependencies. Tauri consumes that same library and retains its old public API
and `agent_worker` example as compatibility wrappers. The carrier currently
targets Linux and macOS.

Applications can build this binary or install it to an application-owned prefix:

```sh
cargo install --path hosts/registered --bin leselang-agent-worker --locked --offline --profile lean --target-dir target --root /absolute/application-prefix
```

Configure the Node transport with the resulting absolute
`/absolute/application-prefix/bin/leselang-agent-worker` path. The SDK does not
download or install a runtime implicitly. Source dependencies are repository
local; this is a local build/install flow, not a published binary distribution.

## Compose An Application Entry

An independent application imports both entries from the installed package:

```ts
import { randomUUID } from "node:crypto";
import { AgentSession, createRegisteredAgentHost, createSemanticProvider } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";

// application and addApplicationSemantics belong to your application.
const transport = createNodeAgentTransport({
  executable: "/absolute/path/to/leselang-agent-worker",
  catalog: addApplicationSemantics,
});
const host = createRegisteredAgentHost({
  window: "my-application", epoch: randomUUID(), application, transport,
  observations: { before: "objects_before", after: "objects_after" },
});
const session = new AgentSession({ host, provider: createSemanticProvider() });
try {
  const outcome = await session.submit("task my_declared_task");
  console.log(outcome.status);
} finally {
  session.cancel();
  host.close();
}
```

`NodeAgentTransportOptions` is exported from the same Node subpath. Its
`catalog(baseJson: string): string` callback synchronously enriches the real
worker's base catalog; `args?: readonly string[]`, `timeoutMs?: number` and
`onStderr?: (text: string) => void` configure delivery. The returned value is a
`RegisteredAgentTransport`. No Node-specific types leak into that public
declaration. Node 24 is the tested runtime, and the carrier uses only built-in
modules. The root browser entry does not import or re-export it.

The package remains private and is installed from a local build or packed
tarball. The tarball contains both public entries and declarations, with no
runtime npm dependencies or worker binary. No copying of the carrier source
into each application is required.

The small reference composition can be used directly from a repository-root
Node ESM program:

```js
import { createPublicationAgent } from "./consumer/javascript/agent/assistant.mjs";

const assistant = createPublicationAgent();
try {
  const published = await assistant.session.submit("task publish_article:Hello from Node");
  const read = await assistant.session.submit("task read_publication");
  console.log(published.status, read.status, assistant.application.snapshot());
} finally {
  assistant.close();
}
```

Each description launches `leselang-agent-worker --describe` and reads its actual base
operation catalog. The application adds its own semantic declarations and
validates them through the public SDK. The worker does not choose an application
task or provide a business-verification claim.

Use [`defineSemanticCatalog`](semantic-authoring.md) to compose that base with
typed v1/v2 declarations. The reference application now declares its four tasks
directly and shares the publication definition with the legacy v1 Consumer;
it does not parse and modify a completed catalog to upgrade its semantics.
Its two semantic author modules and application adapter also pass strict
TypeScript `checkJs` using public types.

The v2 entry uses [`defineRegisteredTask`](registered-tasks.md) for common
registered-control sequences. It retains explicit inventory, editable-draft,
published-value and publication-count checks. The legacy v1 declaration stays
unchanged. The constructor emits the same semantic vocabulary, so the existing
provider, worker and independent verifier need no application-specific branch.

| Declared task | Input | Verified result | VM Effects on success |
| --- | --- | --- | --- |
| `read_draft` | None | Observed current draft string | 2 |
| `publish_article` | Named string `text` | Boolean `true`, plus publication and preservation predicates | 9 |
| `read_publication` | None | Observed actual published string | 2 |
| `check_publication` | None | `none`, with all five observations unchanged | 1 |

Read and check contracts compare complete before/after records for all five
objects. Publication checks the submitted text, publication count and status,
while preserving unrelated controls. The public host additionally compares the
report with independent application observations captured before execution and
at verification time. A worker success alone does not satisfy those checks.

The registered host exposes both Boolean `verify` and `verifyDetailed` through
the public SDK. Sessions select the detailed hook once and retain bounded
`verificationFailure` reasons on failure, including report correlation and fresh
application-state mismatches. Both host entry points consume the same one-use
completion evidence; do not call them in succession for one execution. See
[verification diagnostics](verification-diagnostics.md) for the cause groups and
unchanged stop, partial-effect and no-automatic-retry behavior.

For another application, declare its controls with `createRegisteredApplication`
and provide its own semantic catalog builder. The reference
[`application.mjs`](../consumer/javascript/agent/application.mjs) uses this helper
for attachment, cleanup and fresh observations. Its handlers still update the
retained model through `context.commit` and explicitly notify dependent controls
with `context.changed`. A custom `attach`, `detach` and `observe` connection
remains supported. The Node carrier accepts a trusted
absolute executable path, optional fixed argument prefix and catalog callback.
These belong to application configuration. Model output must not choose an
executable, shell command or process arguments. Source and typed task arguments
are sent as separate fields over stdin; `spawn` runs with `shell: false`.

## Worker Protocol And Bounds

The private line-oriented carrier defaults to `agent_worker_version: 1`. Plans
with explicit `inputMode: "named_strings"` use version 2; every record in that
execution must retain its selected version, without fallback. See
[v3 named task inputs](named-task-inputs.md). `--serve`
accepts one initial `start` record containing `scope`, `source` and `arguments`.
Complete syntax, type and argument admission occurs before the first request
for a registered object. Subsequent `request` and `reply` records carry a fresh
64-character hexadecimal challenge plus the existing Bridge message. Both sides
check the pending request and complete window/epoch/session/request identity;
unsolicited, duplicate and foreign delivery fails the execution.

One terminal `report` record contains the original report JSON in the
`report_json` string. An `error` record instead reports bounded failure text.
The worker fixes the report profile to `javascript.registered`, kind to
`javascript_task_run`, and observation fields to `objects_before` /
`objects_after`. Neither a provider nor the startup record selects these values.
Reports preserve typed scalar results and exact `u64` bytes; the carrier does
not parse result integers through JavaScript Number.

| Boundary | Limit |
| --- | --- |
| Startup JSON line | 128 KiB |
| Source | 16 KiB, with no NUL |
| Task inputs | v1: zero or one string; v2: up to eight. Each is at most 4096 UTF-8 bytes, with no NUL; v2 values total at most 16 KiB |
| Request/reply envelope | 64 KiB; the inner gateway message still has its existing 32 KiB limit |
| Original report JSON | 128 KiB |
| Terminal envelope / accumulated incomplete frame | 512 KiB |
| Base catalog | 256 KiB |
| Worker stderr | 128 KiB |
| Execution stdout total | 256 request envelopes plus one terminal-frame budget |
| Worker startup or request interval | 5 seconds, including bounded request write/read waits |
| Parent whole-process deadline | 15 seconds by default; trusted configuration may select 20–60,000 ms |

Decoders reject duplicate JSON fields, invalid UTF-8, malformed shapes and
incomplete frames. Envelope numbers must fit the carrier's integer domains.
The Node parent only resolves execution after exactly one complete terminal
report, no trailing data, child `close`, and exit code zero. A report followed
by a hang, nonzero exit or another frame is not accepted. Raw report bytes then
pass the SDK's semantic and independent application checks.

The Rust worker retains explicit `poll → authorize → dispatch → accept`, its
10,000-Fuel budget and the original registered-object gateway. Bootstrap and
final host observations are additional transport reads outside VM Effect/Fuel
accounting. Pipe reader/writer threads provide bounded delivery; they do not
add a language scheduler. This stdio reference is not a remote or durable
protocol and supplies no restart recovery or persisted authority.

## Cancellation And Failure

Cancel through `AgentSession.cancel()` or `AgentWorkflowSession.cancel()` so the
public host closes the Bridge mutation gate immediately. The transport then
terminates only the matching worker and waits for actual child closure before
settling the execution. Adapters must reenter `context.commit` after awaits.
Late completion cannot mutate through that retired Bridge, and old replies or
cancel requests cannot terminate a later execution's worker.

A process failure is a different event from explicit SDK cancellation. On a
carrier deadline or malformed output, the transport starts terminating the
worker and rejects when the child closes. During that interval, a previously
authorized JavaScript callback can still reach its commit gate before the host
observes settlement. Killing Rust does not itself revoke JavaScript authority.
Such a failure must preserve possible partial effects; it must never be treated
as proof that nothing happened. After settlement, the host retires the Bridge
and subsequent late commits are refused.

`--fault-after-commit` demonstrates the complementary case: the application
mutation has already happened before the handler throws. Its state survives the
failed worker, and a later separately requested read can inspect it. Neither
carrier nor workflow silently retries publication.

## Sources And Verification

| Source | Responsibility |
| --- | --- |
| [`agent.mjs`](../consumer/javascript/agent.mjs) | Explicit offline four-step CLI demo |
| [`assistant.mjs`](../consumer/javascript/agent/assistant.mjs) | Public SDK and application composition |
| [`application.mjs`](../consumer/javascript/agent/application.mjs) | Parent-owned model, registrations and independent observations |
| [`catalog.mjs`](../consumer/javascript/agent/catalog.mjs) | Application-authored task semantics |
| [`node/index.ts`](../packages/javascript/src/node/index.ts) | Public optional Node entry and configuration types |
| [`node/transport.ts`](../packages/javascript/src/node/transport.ts) | Node child lifecycle and bounded stdio delivery |
| [`worker.rs`](../hosts/registered/src/worker.rs) | One admitted Rust task with fixed registered report identity |
| [`task.rs`](../hosts/registered/src/task.rs) | Shared typed task admission and VM execution |
| [`agent_worker.rs`](../hosts/tauri/examples/agent_worker.rs) | Retained compatibility executable delegating to the standalone implementation |

The `node-consumer` group includes real VM integration and an external Consumer
that installs the packed SDK into a fresh directory before executing success,
post-commit failure and cancellation through the real worker. The `javascript`
group exercises separate malformed-worker fixtures, declaration consumers and
browser import isolation. The original [`node_host`](../hosts/tauri/examples/node_host.rs)
still starts the older independent JavaScript adapter and retains its existing
Boolean publication contract. These two directions coexist; no existing
Consumer is replaced. SDK tests and the Tauri applications continue to exercise
the same public registered host independently. Dated results belong in the
[Node-owned agent acceptance record](acceptance/agents.md#node-owned-agent-with-real-vm-workers)
and [installed public Node Consumer record](acceptance/agents.md#public-node-transport-and-installed-consumer),
followed by the [standalone installed worker record](acceptance/agents.md#standalone-registered-host-and-installed-worker),
separately from these setup and contract instructions.
