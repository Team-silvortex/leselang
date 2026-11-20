# @leselang/bridge

A small ESM package for trusted JavaScript and TypeScript applications that
register objects controlled by a LeseLang host. It has no runtime dependencies.
The root entry does not depend on Tauri, a browser, Node APIs or a UI framework;
the optional `@leselang/bridge/node` entry uses Node's built-in process APIs.
The package is private while its transport contract is being developed.

## Register An Object

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

## Single-Task Agent Entry

The same public entry exports `AgentSession`, `AgentProvider`, `AgentHost` and
`WorkflowPlan` for an application-owned conversation entry. A session explicitly
describes host capabilities, asks the provider for source and literal arguments,
executes through the host, and requires independent application verification.
It admits one submission at a time, retains that slot until outstanding work
settles after cancellation, and refuses late plans or results. It does not
contact a model itself, manage credentials/history, retry a workflow or grant
authority from provider output. The repository's `docs/agent-entry.md` describes
the interactive reference and the replaceable provider contract.

For registered applications, `createRegisteredApplication` accepts a static map
of control declarations and supplies the `attach`, `detach` and fresh `observe`
connection. Its `RegisteredControl` handlers retain explicit `context.commit`;
`RegisteredActionContext.changed(...names)` notifies dependent controls only
inside that synchronous commit and remains bound to its original execution.
Use the returned application's `changed(...names)` for manual updates to the
currently attached execution. The helper owns registration and cleanup, while
the application owns its model, DOM checks and actual actions.

The optional `createRegisteredAgentHost` supplies
the host implementation from this application connection and a trusted execution
transport. It admits semantic plans, owns each Bridge lifetime and matches the
original execution report against independent before/current application
observations. It has no Tauri dependency and adds no VM scheduler. The original
`AgentHost` and `Bridge` APIs remain available. See the repository's
[registered application host guide](../../docs/registered-agent-host.md) for the
public adapters, report layout and cancellation contract.

Hosts can opt into `AgentHost.verifyDetailed`; the session uses that hook once
and preserves a bounded `verificationFailure` on failed outcomes and states.
Existing Boolean `verify` hosts keep their original result shapes. The public
`VerificationResult`, `VerificationDiagnostic` and `VerificationFailureCode`
types describe the new records. See [verification diagnostics](../../docs/verification-diagnostics.md)
for cause codes, one-use completion evidence and unchanged authorization limits.

## Optional Node Worker Transport

Node applications can import `createNodeAgentTransport` and
`NodeAgentTransportOptions` from `@leselang/bridge/node`. This entry owns the
bounded stdio protocol and child lifecycle for the standalone Rust `leselang-agent-worker`:

```ts
import { createNodeAgentTransport } from "@leselang/bridge/node";

const transport = createNodeAgentTransport({
  executable: "/absolute/path/to/leselang-agent-worker",
  catalog(baseJson) {
    // Add the application's semantic tasks to the worker's base description.
    return addApplicationSemantics(baseJson);
  },
  timeoutMs: 15_000,
});
```

Pass this transport to `createRegisteredAgentHost` with your registered
application and `observations: { before: "objects_before", after: "objects_after" }`.
`catalog` is a synchronous callback returning raw JSON text. Optional `args`
are a fixed argument prefix; optional `onStderr` receives bounded diagnostics.
Executable and arguments are trusted application configuration. The package
does not install a Rust binary or download one at runtime.

Each task gets a fresh worker while the application retains its model. The
transport returns an original report only after complete framing and clean
child closure, and waits for closure after cancellation. Call session
`cancel()` to immediately retire the application's mutation gate. Completed
effects survive failures; neither the carrier nor the session retries them.
See the repository's [Node host guide](../../docs/node-agent-host.md) for the
full composition, protocol bounds and real-worker acceptance.

The subpath's public declarations use the same portable types as the root
entry. Consumers do not need `@types/node` just to use this API. Browser code
keeps importing `@leselang/bridge`; the Node entry is never re-exported there.

## Semantic Tasks And Observed Results

`defineRegisteredTask` constructs reusable registered-control task declarations
from `read_control`, `set_control_text` and `click_control` steps, mixed with
ordinary semantic calls and guards. `RegisteredTaskDefinition` supplies the
explicit inputs, result, guard failure and report verification; the returned
v2 declaration is owned and frozen. The constructor adds the usual observation,
actionability, revision and acknowledgement sequence, while the application
still states its business success conditions. Pass the result to
`defineSemanticCatalog` for validation against the actual base catalog. See the
[registered task guide](../../docs/registered-tasks.md) for a complete example.

Applications can author catalogs through `defineSemanticCatalog(base, semantics)`
and the public readonly `SemanticDescription` types. The helper validates the
complete declaration against the real base catalog and returns bounded raw JSON,
with exact `bigint` support for authored unsigned integers. It rejects an already
semantic base instead of silently replacing its task contract. v1/v2 declarations
retain their existing wire shapes and runtime checks. See the repository's
[semantic authoring guide](../../docs/semantic-authoring.md) for TypeScript and
checked JavaScript examples.

`createSemanticProvider()` supplies a reusable offline command provider for
`task TASK:input` and `task TASK` (no input). It reads `semantics v1` or `v2` tasks from
the host catalog instead of selecting a built-in form workflow. Applications
may configure local aliases, for example
`createSemanticProvider({ aliases: { save: "publish_article" } })`.
`prepareSemanticCatalog(rawJson)` exposes the same validated task compiler and
report verifier directly. Its `tasks` array describes available commands;
`prepare(id, arguments)` returns a frozen `plan`, `expectedEffects`, `checks`
and both `verify(report)` and `verifyDetailed(report)` functions. The former
retains its Boolean contract; the latter returns a structured verification result.
A `WorkflowPlan` may include the declared `task`
and zero or one named string input; existing `{ text: value }` plans remain valid.

Semantic v3 separately supports up to eight named string inputs. Use
`SemanticDescriptionV3` / `SemanticTaskDescriptionV3` and optionally
`defineRegisteredTaskV3`. Its plans include `inputMode: "named_strings"`, and
its task info exposes `inputNames`. The offline provider accepts
`task TASK:{"title":"text","content":"text"}` only for v3; old colon text is
still literal. Workflow bindings can populate several inputs independently.
The Node carrier selects worker protocol v2 for these plans, while legacy plans
retain v1. See [named task inputs](../../docs/named-task-inputs.md) for bounds and
the corresponding Rust/Tauri entries.

Version 1 fixed results and APIs remain unchanged. A host can separately opt into
`semantics.version: 2`, whose `verification.result` may be
`{kind: "observed", scalar_type: "string", path: [...]}`. This contract requires
the actual typed result to equal the selected actual report observation, alongside
all existing predicates, header, Fuel and Effect checks. Its path must start at
a non-`result` field; only string, integer and Boolean values are supported.
It is not a type-only return contract. The application must still independently
verify fresh state. See the repository's `docs/host-semantics.md` for the complete
versioned contract and Rust publication API.

## Model Task Selection

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

## Sequential Workflows And Result Bindings

`AgentWorkflowSession` adds bounded sequential execution without changing the
single-task APIs. Its `AgentWorkflowProvider` returns `{steps: WorkflowPlan[]}`
with one to eight semantic tasks. The session validates every task, argument
and canonical program before executing any step, pins the complete catalog,
and rechecks a fresh description before each step. It awaits independent host
verification before proceeding. Failure or cancellation stops the remaining
steps and retains earlier reports and possible side effects; it does not retry
or roll back.

`prepareSemanticWorkflow(rawCatalog, steps)` returns a canonical workflow plan
from `SemanticWorkflowStep` declarations. Each argument can be a literal string
or a `WorkflowResultReference`, for example `{fromStep: 0, format: "decimal"}`.
References must point backward and use an explicit matching conversion:
`string` for a string result, `decimal` for an exact `u64`, or `boolean` for
`"true"`/`"false"` text. `none` and other result kinds are rejected. The helper
normalizes a reference into an empty target argument template and an immutable
`AgentWorkflowBinding` in optional `plan.bindings`.

The session admits every binding and source/target type before any execution.
A referenced source must pass both its semantic report verifier and the host's
application verifier. The later literal comes from the actual accepted result,
not the catalog's expected literal; integer conversion is lossless. The aggregate
outcome retains symbolic bindings, while each attempted step's plan records its
resolved argument. There are no arbitrary report paths, expressions, branches
or implicit conversions, and cancellation still prevents dependent execution.
For an observed v2 source, conversion admission uses its declared `scalar_type`.
The source result may vary between executions, but it must match each run's
declared observation before it can be bound into a later input. For example,
`read_message` can supply current input text to `save_message` with string format
without the planner knowing that text in advance.

Each `SemanticWorkflowStep` may also have one optional `requires`, such as
`{fromStep: 0, format: "string", equals: "expected text"}`. This public
`WorkflowResultRequirement` becomes an `AgentWorkflowRequirement` in optional
`plan.requirements`, with the target `step` added. It uses the same explicit
scalar formats and backward reference rules as bindings, independently of
whether that step has a bound argument. Decimal expectations are canonical
`u64` strings and Boolean expectations are exactly `"true"` or `"false"`.

The complete requirement graph is admitted before execution. After a fresh
matching catalog description, the target continues only if the source's actual
semantically and application-verified result equals the expected text. Mismatch
returns `workflow_requirement_not_met` before target execution and stops later
work. Optional `outcome.requirements` records only comparisons actually made as
`AgentWorkflowRequirementOutcome` values with `actual` and `satisfied`; earlier
reports and possible effects remain. This does not skip steps, retry actions or
make observation and action atomic. Live state and permission checks still
belong to the host.

`createModelWorkflowProvider({ complete })` uses the same model transport but
accepts `{steps: [{task, arguments}, ...]}` or `{error}`. Its
`ModelWorkflowProviderOptions.resultBindings` flag defaults to `false`. When
explicitly enabled, argument values may also use the inline result-reference
form above; the model cannot supply a top-level `bindings` field. The independent
`resultRequirements` option also defaults to `false`; enabling it permits the
inline `requires` object above, but no top-level `requirements` or model-supplied
comparison evidence. The combined response is limited to 16 KiB for v1/v2 and
128 KiB for v3; an application's transport can impose a smaller limit. The Tauri
conversation explicitly enables both options for this provider in model mode
and adapts each offline command into a one-step workflow. Each step keeps the
ordinary host session and Fuel behavior. See the repository's
`docs/agent-workflows.md` for the progress and outcome contracts, catalog pinning
and verification scope. Existing literal workflow plans and single-task provider
contracts remain compatible.

## Report Integrity

Pass original JSON text when catalogs or reports contain `u64` integers.
The bounded semantic decoder rejects duplicate keys and preserves exact
integers internally without exposing BigInt in plans or IPC. Already-parsed
objects must contain safely representable numbers. Catalogs are limited to
256 KiB and reports to 128 KiB. Hosts should keep their own admitted catalog,
rebuild and compare a provider's proposed plan before execution, and combine
the declared report checks with fresh application observations. The metadata
does not grant authority, prove its author's semantics, or alter runtime Fuel.

## Object Lifetimes And Mutation Gates

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

## Build And Verify The Package

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
WebView assets during its build, preserving the browser module tree and
excluding the opt-in Node transport.

## Source Organization

Within the repository, `src/index.ts` and `src/node/index.ts` define the root
and optional Node public entries. The implementation modules below are not
additional package subpaths:

| Path under `src/` | Responsibility |
| --- | --- |
| `bridge.ts` | Registered-object protocol |
| `node/transport.ts` | Optional Node child lifecycle and bounded worker delivery |
| `agent/session.ts`, `agent/data.ts` | Single-task lifetime and owned bounded data |
| `agent/registered-host.ts`, `agent/registered-evidence.ts` | Optional application host, transport lifecycle and independent observations |
| `agent/registered-application.ts` | Optional static control declarations, connection lifetime and scoped change notifications |
| `agent/workflow.ts`, `agent/workflow-bindings.ts` | Sequential tasks, verified-result dependencies and requirements |
| `providers/semantic.ts`, `providers/model.ts` | Offline command and model selection providers |
| `semantics/index.ts` | Semantic API assembly |
| `semantics/declarations.ts`, `semantics/authoring.ts` | Public declaration types and validated, lossless catalog composition |
| `semantics/registered-task.ts` | Common registered-control steps expanded into ordinary task declarations |
| `semantics/{json,catalog,source,report}.ts` | Lossless decoding, catalog admission, canonical programs and report checks |

The build clears only this package's generated `dist/` through `clean.mjs` before
emitting the complete ESM/declaration tree. Packed-package tests verify relative
imports from an external installation. Source organization does not change the
public package entry or the semantics of existing single-task and workflow APIs.
