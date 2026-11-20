# @leselang/bridge

A small ESM package for trusted JavaScript and TypeScript applications that
register objects controlled by a LeseLang host. It has no runtime dependencies
and does not depend on Tauri, a browser, Node-specific APIs or a UI framework.
The package is private while its transport contract is being developed.

```ts
import { Bridge } from "@leselang/bridge";

const bridge = new Bridge({
  window: "application", epoch: "instance-1", session: "task-1",
});
let count = 0;
bridge.register("counter", {
  read: () => ({
    role: "button", title: "Increment", value: String(count),
    enabled: true, editable: false, clicks: count,
  }),
  async click(context) {
    await Promise.resolve();
    context.commit(() => { count += 1; });
  },
});
```

The application transport explicitly calls `await bridge.handle(request)` and
returns its reply. The bridge does not poll the VM, select operations, run a
scheduler or evaluate agent-provided JavaScript. Rust retains the VM, Fuel,
permission grants and its local object/reply identities. JSON carries bounded
request and observation data, not those Rust authorities.

The same public entry exports `AgentSession`, `AgentProvider`, `AgentHost` and
`WorkflowPlan` for an application-owned conversation entry. A session explicitly
describes host capabilities, asks the provider for source and literal arguments,
executes through the host, and requires independent application verification.
It admits one submission at a time, retains that slot until outstanding work
settles after cancellation, and refuses late plans or results. It does not
contact a model itself, manage credentials/history, retry a workflow or grant
authority from provider output. The repository's `docs/agent-entry.md` describes
the interactive reference and the replaceable provider contract.

`createSemanticProvider()` supplies a reusable offline command provider for
`task TASK:input` and `task TASK` (no input). It reads `semantics v1` tasks from
the host catalog instead of selecting a built-in form workflow. Applications
may configure local aliases, for example
`createSemanticProvider({ aliases: { save: "publish_article" } })`.
`prepareSemanticCatalog(rawJson)` exposes the same validated task compiler and
report verifier directly. Its `tasks` array describes available commands;
`prepare(id, arguments)` returns a frozen `plan`, `expectedEffects`, `checks`
and `verify(report)` function. A `WorkflowPlan` may include the declared `task`
and zero or one named string input; existing `{ text: value }` plans remain valid.

`createModelProvider({ complete })` uses an application-owned model transport to
select a declared task from natural language. The injected function receives
`ModelRequest` (`system` and `user` strings) plus an `AbortSignal` and returns
the model's complete JSON text. It must return exactly `{task, arguments}` or
`{error}`. The SDK validates the selection and generates source with the same
catalog compiler; model-supplied source, verification claims and extra fields
are rejected. It does not repair replies, retry or fall back to an offline
provider. Credentials, network deadlines and actual request cancellation belong
to the application transport. Each submission is independent and selects one
declared task; this interface does not supply history or compose new tasks.

```ts
import { AgentSession, createModelProvider } from "@leselang/bridge";

const provider = createModelProvider({ complete: applicationModelTransport });
const session = new AgentSession({ provider, host });
const outcome = await session.submit("请把“周五完成设计评审”保存到应用中。");
```

The model receives the user message, task summaries and full host catalog.
The application decides where that data is sent. A valid model selection still
requires independent host admission and verification; verification of a selected
task does not prove the model understood the user's intent. For the native Tauri
transport and configuration, see the repository's `docs/model-provider.md`.

`AgentWorkflowSession` adds bounded sequential execution without changing the
single-task APIs. Its `AgentWorkflowProvider` returns `{steps: WorkflowPlan[]}`
with one to eight semantic tasks. The session validates every task, argument
and canonical program before executing any step, pins the complete catalog,
and rechecks a fresh description before each step. It awaits independent host
verification before proceeding. Failure or cancellation stops the remaining
steps and retains earlier reports and possible side effects; it does not retry,
roll back or substitute earlier results into later arguments.

`createModelWorkflowProvider({ complete })` uses the same model transport but
accepts `{steps: [{task, arguments}, ...]}` or `{error}`. The combined response
still has a 16 KiB limit. The Tauri conversation uses this provider in model mode
and adapts each offline command into a one-step workflow. Each step keeps the
ordinary host session and Fuel behavior. See the repository's
`docs/agent-workflows.md` for the progress and outcome contracts, catalog pinning
and verification scope.

Pass original JSON text when catalogs or reports contain `u64` integers.
The bounded semantic decoder rejects duplicate keys and preserves exact
integers internally without exposing BigInt in plans or IPC. Already-parsed
objects must contain safely representable numbers. Catalogs are limited to
256 KiB and reports to 128 KiB. Hosts should keep their own admitted catalog,
rebuild and compare a provider's proposed plan before execution, and combine
the declared report checks with fresh application observations. The metadata
does not grant authority, prove its author's semantics, or alter runtime Fuel.

The protocol's historical `window` field identifies a named application
endpoint; Node and worker integrations do not need a window. `window`, `epoch`
and `session` are exact scope labels of 1–64 ASCII letters, digits, hyphens or
underscores. An integration binds that scope to its own trusted transport
lifetime. A Tauri integration can impose a stricter `main` WebView policy.

`register`, `replace` and `unregister` maintain object identity. `changed(name)`
records external changes and dependencies, including ABA changes that return
to the same visible value. Registered names use the same label rules; at most
64 names can be allocated during a bridge lifetime. The initial operations are
`gui.list`, `gui.find`, `gui.observe`, `gui.set_text` and `gui.click` for the
registered text-field and button adapters.

Every asynchronous adapter must put each application mutation inside a
synchronous `context.commit` callback, including after an await. The gate
rechecks the current scope lifetime, grant, object generation, observed revision
and request deadline. `context.signal` also supports cooperative cancellation.
`cancel(session)`, `revoke()` and `close()` refuse later commits and requests.
An already completed mutation is never rolled back or retried automatically.
An adapter failure after callback entry returns an uncertain outcome.

This is a cooperation contract for trusted application adapters, not a JavaScript
sandbox. Direct mutations outside the gate cannot be prevented, and JavaScript
computation is not metered by LeseLang Fuel. A remote transport closing and the
local bridge receiving cancellation are separate events; the host must retain
partial-effect semantics during that interval. The protocol admits one pending
action at a time and rejects duplicate, skipped, foreign or stale requests.

Build and test independently with Node.js 24 and the pinned TypeScript tool:

```sh
npm ci --ignore-scripts
npm test
```

Tests use the compiled public export. The package test packs the artifact,
installs that tarball into an external temporary consumer offline, executes its
JavaScript through the public import and checks its TypeScript declarations.
Only compiled ESM, declarations, this guide and the MIT license are packaged.

For an application integration see the repository's `consumer/javascript`
example, `docs/javascript-sdk.md` and `docs/typescript-host.md`. The Tauri form
uses this package and copies its installed public ESM entry into the bundled
WebView assets during its build, preserving the complete relative module tree.

Within the repository, `src/index.ts` is the public export boundary.
`bridge.ts` implements the object protocol, `agent.ts` coordinates task
lifetimes, `workflow.ts` coordinates bounded sequences of semantic tasks,
`semantic-provider.ts` selects declared tasks from commands, and
`model-provider.ts` admits model-selected tasks from an injected transport. `semantics.ts`
assembles the semantic API from private `semantics/json.ts`, `catalog.ts`,
`source.ts` and `report.ts` modules. Their distribution files are needed by
relative ESM imports; they are not additional public package subpaths.
