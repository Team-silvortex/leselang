# An Agent Entry Inside A JavaScript Application

An application can expose a conversation entry where a user requests work,
an agent provider proposes a LeseLang workflow, and the application executes
and verifies it through its registered operations. The application owns the
provider, its credentials and conversation context. LeseLang supplies the typed
execution boundary; the JavaScript SDK coordinates one explicit submission at a time.

The [Tauri reference application](typescript-host.md) demonstrates this entry
with selectable model and offline planners backed by the host's versioned semantic
catalog. The [model provider](model-provider.md) accepts natural language through
an application-configured service, then generates source for one or more selected
declarations. The page uses the bounded [workflow session](agent-workflows.md)
to execute and verify them in order. Without model configuration, the reference uses the clearly
labelled offline provider. The shared `createSemanticProvider` accepts `task TASK:input` (or
`任务 TASK：input`), and `task TASK` for a task with no input. These are the
v1/v2 commands; v3 named inputs use a JSON map, for example
`task publish_article:{"title":"标题","content":"正文"}`. The reference app
adds `保存：…` and `save:…` as local aliases for its declared `fill_and_save`
task. This offline provider makes no model call and does not interpret unrestricted natural language.
Application-specific operation names, control names, guards and verification
predicates come from the catalog rather than from the provider.

The [task board reference](taskboard-host.md) uses the same conversation module,
providers and host verifier with a second application's registrations and
catalog. It has no text form: it observes task status and completes a pending
item once, then disables its control. Native startup pins each application's
page and report identity; the original form entry remains compatible.

## Responsibilities

```text
User message
  → application chat UI
  → host capability description
  → AgentProvider: candidate LeseLang source + typed arguments
  → host compiler, authorization and VM
  → registered JavaScript operations
  → fresh application observations
  → application verification
  → result shown in the conversation
```

The provider receives the user message and the actual host catalog. Its output
is a candidate `WorkflowPlan` with `summary`, `source`, an optional declared
`task` identifier, and `arguments` containing the task's named string inputs.
Legacy plans retain zero or one input. Semantic v3 plans explicitly carry
`inputMode: "named_strings"` and accept zero to eight inputs, including an empty
map for a no-input task. User content stays in argument values; it is never substituted into the
LeseLang source. The application remains responsible for choosing and granting
its exposed operations. A plan cannot manufacture a Rust object reference,
permission grant, receipt or verified result.

The host executes with the existing explicit
`poll → authorize → dispatch → pump → accept` sequence. The application verifier
checks both the program result and the observed business postcondition. A
provider's summary or a successful Promise alone is insufficient evidence.

## SDK Contract

The public `@leselang/bridge` entry exports `AgentSession`, `AgentProvider`,
`WorkflowPlan`, `createSemanticProvider`, `createModelProvider`, `prepareSemanticCatalog` and the
associated host and outcome types. Construct a session
with an application-selected provider, host and optional event listener. This
single-task interface remains compatible:

```ts
import { AgentSession, createSemanticProvider } from "@leselang/bridge";

const provider = createSemanticProvider({ aliases: { 保存: "fill_and_save" } });
const session = new AgentSession({ provider, host, onEvent });
const outcome = await session.submit("保存：新的消息");
```

Here `host` and `onEvent` are application implementations. To interpret natural
language, use `createModelProvider({ complete })` with an application-owned
transport; see its [request contract and Tauri configuration](model-provider.md).
A custom provider can also implement the same interface:

- `provider.plan({taskId, message, catalog}, signal)` returns a candidate plan.
- `host.describe(signal)` supplies the current capability catalog.
- `host.execute(plan, signal)` compiles and runs the plan, returning evidence.
- `host.verify(report, plan, signal)` checks the application's postcondition.
- `onEvent` reports the current task phase to the UI.

`submit` returns a verified, failed or cancelled outcome. `cancel()` signals
the active task; `busy` stays true until the current awaited provider or host
operation has actually settled. A second submission cannot overlap that work.
Even a provider that ignores cancellation cannot execute its late plan. A
host result that arrives after cancellation cannot become a verified outcome.

The SDK validates and owns bounded JSON values at the planning and execution
boundaries. Limits include an 8 KiB message, 16 KiB source, 4,096-byte string
argument, 1 KiB summary, 256 KiB catalog and 128 KiB report. V3 additionally bounds
the sum of argument values to 16 KiB. These are UTF-8 limits;
malformed strings, unsupported fields and malformed plans fail before execution.
The SDK does not install a VM scheduler or automatically retry or replan tasks.

The provider may maintain conversation context in an application-owned class
or closure. The SDK does not currently inject chat history into `AgentContext`.
Both reference providers handle each submission independently, while the
application state and visible conversation persist between tasks. Model planning
through `createModelProvider` selects one declared task with its exact named
inputs: zero or one in v1/v2, or zero to eight in v3. The additive
`createModelWorkflowProvider` selects up to eight
declared tasks for `AgentWorkflowSession`; see [sequential workflows](agent-workflows.md).
Neither provider invents new tasks or uses earlier turns as implicit arguments.
The Tauri workflow provider explicitly enables verified result bindings. It can
use an earlier step's accepted string, integer or Boolean result as a later
string argument through a declared conversion. The source must pass semantic
and application verification first; see [result binding](agent-workflows.md#bind-a-verified-result-to-a-later-input).
The ordinary `createModelWorkflowProvider` factory still defaults to literal
arguments unless its application passes `resultBindings: true`.
Tauri also explicitly enables `resultRequirements: true`: a selected task may
require an earlier verified scalar result to equal specified text before it
executes. The default factory rejects this additional vocabulary unless enabled.
See [verified-result requirements](agent-workflows.md#require-a-verified-result-before-continuing)
for type admission, exact comparisons and failure evidence. A failed condition
stops the remaining work; it does not undo earlier effects or lock GUI state.

## Semantic Planning And Host Verification

`prepareSemanticCatalog(rawCatalog)` validates the complete catalog and optional
semantic vocabulary, returning immutable task summaries and a `prepare` method:

```ts
import { prepareSemanticCatalog } from "@leselang/bridge";

const catalog = prepareSemanticCatalog(rawCatalogJson);
const prepared = catalog.prepare("publish_note", { message: "发布内容 ✓" });
// prepared.plan is source plus literal arguments, with no VM authority.
const report = await applicationExecute(prepared.plan);
const declaredConditionsPassed = prepared.verify(report);
```

The semantic API requires a valid semantic v1, v2 or v3 declaration. It has no hidden
form fallback. It supports the same finite call/guard vocabulary and report
predicates as the [Rust catalog agent](host-semantics.md), including typed
fields, unique object selection, integer increases and upper bounds. The task
input remains data. A plan contains no provider-selected verification contract.
Version 1 keeps fixed scalar results. Version 2 additionally permits a typed
result that must equal a declared actual observation in the report, preserving
all other predicates and fresh-state checks; see [observed results](host-semantics.md#version-2-observed-results).
Version 3 retains those result contracts and adds explicit named-string inputs;
see [named task inputs](named-task-inputs.md). Task summaries expose
`SemanticTaskInfo.inputNames` for every v3 task, including an empty array for
no-input tasks. Legacy summaries retain their optional `inputName`. A UI should
use `inputNames` when present to generate the v3 JSON-map command syntax.

Prefer the original JSON text for catalogs and reports. The semantic decoder
checks duplicate keys and preserves integer tokens as internal `BigInt` values;
it never rounds the host's `u64` literals through JavaScript `Number`. Plain
object input is accepted only where numeric values are safely representable.
In particular, `JSON.parse(rawCatalogJson)` loses information for the real
reference catalog's `i64::MAX` predicates and is not an equivalent input.
Internal big integers do not appear in `WorkflowPlan` or cross the execution IPC.

The Tauri host keeps its own validated catalog from `describe`. Before starting
a worker, it independently prepares the selected task and compares source and
arguments with the provider's proposal. It also captures application state
before execution. Verification checks the saved contract, the corresponding
execution report, that initial state and a fresh current observation, matching
records by unique identifier. A self-consistent stale report cannot replace
current application evidence. Provider-supplied source changes or extra
verification fields cannot redefine the host's contract.

The legacy SDK plan and `agent_start_task` carrier support zero or one named
string input. The explicit v3 plan marker selects `agent_start_named_task` for
zero to eight named strings. Both carriers use each declared parameter name, such as `message`, without
renaming it to `text`. Before contacting the application, Rust compiles the
complete program, admits its scalar result type, and binds exactly the required
arguments. Missing, extra, malformed or mismatched arguments fail before
transport bootstrap or VM execution. Each string remains bounded to 4,096 UTF-8
bytes, with no NUL and valid Unicode; v3 values total at most 16 KiB. Other input
types remain outside this carrier contract.

The task carrier returns all six existing scalar types: `integer`, `boolean`,
`string`, `none`, `optional_string` and `string_list`. Native objects and snapshots
cannot leave the worker as final results. Semantic v1's literal-result vocabulary
still covers integer, Boolean, string and `none`. Semantic v2 keeps that fixed
vocabulary and adds observed string, integer and Boolean result contracts.
Semantic v3 retains those contracts and the language's scalar limits.

The additive `agent_start_task` command accepts a named `arguments` map and
returns raw JSON with `kind: "tauri_task_run"`, `profile: "tauri.typescript"`,
the typed `result`, Effect/Fuel counts and `native_before` / `native_after`.
It does not manufacture a business `verified` flag. The task declarations use
`require_verified: false`; the frontend host checks the actual result, all
declared predicates and fresh application observations before reporting success.
Raw report bytes preserve exact `u64` results through IPC and are bound to the
completed execution. Parsing the report as ordinary JavaScript JSON is used
only for independently bounded control observations and display, never for
integer verification. The UI does not display rounded integers as exact values;
the original report remains available in a collapsed detail view.

Existing `agent_start(text)` and its object report remain compatible. Request
`agent_describe` with `{raw: true, taskCatalog: true}` for the original four-task
v1 catalog; omitting `taskCatalog` retains the one-shot catalog. Offline callers
can use `--describe-agent` or Rust `describe_agent()` for that same v1 contract.

The conversation explicitly requests
`{raw: true, taskCatalog: true, observedResults: true}` for its separate five-task
v2 catalog. `observedResults` requires `taskCatalog: true`. Offline callers use
`--describe-agent-observed`, and Rust callers use `describe_agent_observed()`.
The added `read_message` task reads current input text without mutation, with
an observed result contract rather than a fixed expected string. The original
four-task v1 catalog and fixtures remain unchanged. Rust embedders can call
`prepare_task(source, arguments)` before starting a worker, then move the owned
`PreparedTask` into `run_prepared_task`; `run_task` combines those steps. These
functions use the registered-object gateway and do not assume form control names.
For v3 inputs, Rust embedders call `prepare_named_task` and pass the resulting
`PreparedTask` to the same driver. The bundled form and board catalogs remain
v2; their existing GUI procedures and offline commands are unchanged.

## Run The Interactive Reference

Build the frontend and desktop host from the repository root:

```sh
npm ci --prefix packages/javascript --ignore-scripts
npm ci --prefix hosts/tauri --ignore-scripts
npm run build --prefix hosts/tauri
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe-agent
target/lean/leselang-tauri-host --describe-agent-observed
target/lean/leselang-tauri-host --agent
```

This mode requires a macOS graphical session. The `--agent` flag is exclusive;
it cannot be combined with the existing one-shot source, text or report options.
The original form runner, catalog description, independent agent workflow and
Node consumer remain available.

The page contains a conversation panel, an explicit provider selector and the
real application form. The same semantic catalog supplies these tasks. The
following commands use offline mode; configured model mode can select them from
natural-language requests:

| Command | Input | Verified result | Application work |
| --- | --- | --- | --- |
| `task inspect_controls` | None | `integer: 3` | Discover and observe the controls; preserve all state |
| `task read_message` | None | `string`: current input value | Find and observe the input; match the returned string to current report evidence and preserve state (v2 catalog) |
| `task save_message:新的消息` | `message: string` | `string: "saved"` | Set the input and save once; check saved text and unrelated controls |
| `task check_controls` | None | `none` | Check the controls; preserve all state |
| `保存：另一条消息` or `task fill_and_save:另一条消息` | `text: string` | `boolean: true` | Preserve the existing save workflow |

Each task starts from current application state. Control checks use three VM
Effects, `read_message` uses two, and save tasks use nine; bootstrap and final host observations remain
separate. Unsupported tasks and wrong input shapes are rejected by the offline
planner without executing application operations. Available tasks and summaries
come from the host catalog. The page displays the plan, verified return type
and value for each step, and offers cancellation while work is active. Model
mode may plan several of these declared tasks in one request; all steps are
admitted before execution and verified in order. Offline commands remain one-step
submissions. Model failure does not
switch providers or retry a task. See [model planning](model-provider.md) for
service configuration, data transmission and its separate acceptance scope.

## Task And Application Lifetimes

The application stays open between tasks. Every execution creates a fresh
Bridge and Rust VM session under the page's fixed epoch. The model and DOM
persist, but request identities, object owners, permission leases and Fuel do
not cross between task executions. Each new task execution, including each step
in a multi-step workflow, has its own 10,000-Fuel budget.

The Tauri integration exposes separate description, start, reply, cancellation
and page-closure commands restricted to the bundled agent page. Its channel
carries only SDK requests; the start command resolves with the worker's final
report. Rust independently prevents concurrent starts and reused sessions.
The reference page admits at most 64 execution sessions, counting individual
workflow steps; reopen the application
for a fresh page lifetime after that bound is reached.
The frontend keeps the old task active until its worker has settled before
allowing another submission.

A multi-step submission stops when a step fails or is cancelled. Earlier
verified steps and their reports remain in the aggregate outcome; they are not
rolled back. Every step receives a fresh description that must match the
workflow's pinned catalog, including its verification metadata. For admission,
progress, evidence and cancellation details, see [sequential workflows](agent-workflows.md).
When a step binds an earlier result, the aggregate plan retains the symbolic
reference and each attempted step records its actual resolved arguments. A
failed, cancelled or unverified source cannot feed a later task.
For a result requirement, the plan retains the required value and the outcome
records each comparison actually made. A mismatch stops before the target
executes and appears explicitly in the conversation alongside the earlier
verified result. Unvisited requirements do not count as failed comparisons.

Cancellation closes the frontend bridge and retires the matching Rust
transport. These are separate events across runtimes. Deferred JavaScript work
must still enter `context.commit`; an earlier mutation remains an external
side effect. Cancellation and failure never imply rollback. The UI treats an
entered execution as potentially partial and does not silently submit it again.
Page closure or navigation retires the page lifetime and its active task.

For application adapter responsibilities, see the
[JavaScript SDK guide](javascript-sdk.md). For raw transport, object revision
and native GUI boundaries, see the [TypeScript host guide](typescript-host.md).

## Recorded Desktop Acceptance

The dated evidence is preserved in [Application Conversation Entry](acceptance/agents.md#application-conversation-entry).
### Generic Task Entry

The dated evidence is preserved in [Generic Task Entry](acceptance/agents.md#generic-task-entry).

### Original Semantic Provider Entry

The dated evidence is preserved in [Original Semantic Provider Entry](acceptance/agents.md#original-semantic-provider-entry).
