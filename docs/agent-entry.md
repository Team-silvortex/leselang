# An Agent Entry Inside A JavaScript Application

An application can expose a conversation entry where a user requests work,
an agent provider proposes a LeseLang workflow, and the application executes
and verifies it through its registered operations. The application owns the
provider, its credentials and conversation context. LeseLang supplies the typed
execution boundary; the JavaScript SDK coordinates one explicit task at a time.

The [Tauri reference application](typescript-host.md) demonstrates this entry
with a clearly labelled offline planner backed by the host's `semantics v1`
catalog. The shared `createSemanticProvider` accepts `task TASK:input` (or
`任务 TASK：input`), and `task TASK` for a task with no input. The reference app
adds `保存：…` and `save:…` as local aliases for its declared `fill_and_save`
task. It makes no model call and does not interpret unrestricted natural language.
Application-specific operation names, control names, guards and verification
predicates come from the catalog rather than from the provider.

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
`task` identifier, and `arguments` containing zero or one named string input.
User content stays in the argument value; it is never substituted into the
LeseLang source. The application remains responsible for choosing and granting
its exposed operations. A plan cannot manufacture a Rust object reference,
permission grant, receipt or verified result.

The host executes with the existing explicit
`poll → authorize → dispatch → pump → accept` sequence. The application verifier
checks both the program result and the observed business postcondition. A
provider's summary or a successful Promise alone is insufficient evidence.

## SDK Contract

The public `@leselang/bridge` entry exports `AgentSession`, `AgentProvider`,
`WorkflowPlan`, `createSemanticProvider`, `prepareSemanticCatalog` and the
associated host and outcome types. Construct a session
with an application-selected provider, host and optional event listener:

```ts
import { AgentSession, createSemanticProvider } from "@leselang/bridge";

const provider = createSemanticProvider({ aliases: { 保存: "fill_and_save" } });
const session = new AgentSession({ provider, host, onEvent });
const outcome = await session.submit("保存：新的消息");
```

Here `host` and `onEvent` are application implementations; a custom provider
can replace the shared offline provider:

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
argument, 1 KiB summary, 256 KiB catalog and 128 KiB report. These are UTF-8 limits;
malformed strings, unsupported fields and malformed plans fail before execution.
The SDK does not install a VM scheduler or automatically retry or replan tasks.

The provider may maintain conversation context in an application-owned class
or closure. The SDK does not currently inject chat history into `AgentContext`.
The offline reference handles each command independently, while the application
model and visible conversation persist between tasks.

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

The semantic API requires a valid `semantics v1` declaration. It has no hidden
form fallback. It supports the same finite call/guard vocabulary and report
predicates as the [Rust catalog agent](host-semantics.md), including typed
fields, unique object selection, integer increases and upper bounds. The task
input remains data. A plan contains no provider-selected verification contract.

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

The reusable SDK and Tauri task carrier support zero or one named string input.
The carrier uses the declared parameter name, such as `message`, without
renaming it to `text`. Before contacting the application, Rust compiles the
complete program, admits its scalar result type, and binds exactly the required
arguments. Missing, extra, malformed or mismatched arguments fail before
transport bootstrap or VM execution. The string remains bounded to 4,096 UTF-8
bytes; multi-parameter tasks and other input types are outside this v1 contract.

The task carrier returns all six existing scalar types: `integer`, `boolean`,
`string`, `none`, `optional_string` and `string_list`. Native objects and snapshots
cannot leave the worker as final results. Semantic v1's literal-result vocabulary
still covers integer, Boolean, string and `none`; this carrier change does not
extend that schema or the language's scalar limits.

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
`agent_describe` with `{raw: true, taskCatalog: true}` for the new task contracts;
omitting `taskCatalog` retains the original catalog. Offline callers can use
`--describe-agent` or Rust `describe_agent()`. Rust embedders can call
`prepare_task(source, arguments)` before starting a worker, then move the owned
`PreparedTask` into `run_prepared_task`; `run_task` combines those steps. These
functions use the registered-object gateway and do not assume form control names.

## Run The Interactive Reference

Build the frontend and desktop host from the repository root:

```sh
npm ci --prefix hosts/tauri --ignore-scripts
npm run build --prefix hosts/tauri
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe-agent
target/lean/leselang-tauri-host --agent
```

This mode requires a macOS graphical session. The `--agent` flag is exclusive;
it cannot be combined with the existing one-shot source, text or report options.
The original form runner, catalog description, independent agent workflow and
Node consumer remain available.

The page contains a conversation panel and the real application form. The same
provider selects each of these declared tasks:

| Command | Input | Verified result | Application work |
| --- | --- | --- | --- |
| `task inspect_controls` | None | `integer: 3` | Discover and observe the controls; preserve all state |
| `task save_message:新的消息` | `message: string` | `string: "saved"` | Set the input and save once; check saved text and unrelated controls |
| `task check_controls` | None | `none` | Check the controls; preserve all state |
| `保存：另一条消息` or `task fill_and_save:另一条消息` | `text: string` | `boolean: true` | Preserve the existing save workflow |

Each task starts from current application state. Read-only tasks use three VM
Effects, and save tasks use nine; bootstrap and final host observations remain
separate. Unsupported tasks and wrong input shapes are rejected by the offline
planner without executing application operations. Available tasks and summaries
come from the host catalog. The page displays the plan, verified return type
and value, and offers cancellation while work is active.

## Task And Application Lifetimes

The application stays open between tasks. Every execution creates a fresh
Bridge and Rust VM session under the page's fixed epoch. The model and DOM
persist, but request identities, object owners, permission leases and Fuel do
not cross between workflows. Each new workflow has its own 10,000-Fuel budget.

The Tauri integration exposes separate description, start, reply, cancellation
and page-closure commands restricted to the bundled agent page. Its channel
carries only SDK requests; the start command resolves with the worker's final
report. Rust independently prevents concurrent starts and reused sessions.
The reference page admits at most 64 workflow sessions; reopen the application
for a fresh page lifetime after that bound is reached.
The frontend keeps the old task active until its worker has settled before
allowing another submission.

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

### Generic Task Entry

On 2026-11-13 the rebuilt macOS WebView displayed all four declarations from
the new task catalog. The same conversation entry then generated, executed and
verified these workflows in sequence:

| Submission | Observed outcome |
| --- | --- |
| `task inspect_controls` | Verified `integer: 3`; no Save or Other action |
| `task save_message:命名参数 "message" \\ 路径` plus a newline and `通用返回值 ✓` | Verified `string: "saved"`; literal input and saved content matched; Save count became one |
| `task check_controls` | Verified `none`; saved content and all counts stayed unchanged |
| `保存：兼容入口 ✓` | Verified `boolean: true`; Save count became two |
| `task inspect_controls:不应接受参数` | Rejected before execution: the task takes no input |
| `task save_message` | Rejected before execution: the required input was missing |

The final input and saved value were `兼容入口 ✓`, Save count was two and
Other remained zero. The Accessibility tree showed all four verified typed
results and both admission failures; the page was also inspected visually.
The worker trace recorded 24 VM Effects across the four executed tasks
(3 + 9 + 3 + 9). The two rejected submissions added no Effects. Each workflow
used the same shared provider and an independently described host contract.

The automated Rust tests additionally execute all six scalar result types,
including exact `u64::MAX`, through the real VM; test signature admission before
any transport request; and compare legacy/new Fuel and Effect counts for the
same program. Cancellation, late replies and partial mutations are covered by
the transport and frontend suites, not by manually timed desktop cancellation.

### Original Semantic Provider Entry

On 2026-11-13 the rebuilt macOS `--agent` WebView was exercised through its
conversation UI with the shared semantic provider:

1. The available task list displayed `fill_and_save` and its actual
   host-declared summary before a workflow was submitted.
2. `task fill_and_save:目录驱动的工作流 ✓` produced a verified result and one Save.
3. `保存：第二轮 "引号" \\ 路径` followed by a newline and `语义目录 ✓` produced
   another verified result through the application alias. The input and saved
   content matched exactly, the accumulated Save count became two, and Other
   remained at zero.
4. `task missing_task:不应执行` was rejected because the host had not declared
   that task. The conversation stated that application operations had not
   started; the saved value and counts remained unchanged.

These results were observed in the native Accessibility tree of
`tauri://localhost/agent.html`; the loaded page was also inspected visually.
Cancellation and late-completion coverage comes from SDK, frontend connection
and Rust registry tests. This desktop record does not claim a manually timed
cancellation or a connection to a live model service.

The same public provider also passed the independent Node Consumer's
`publish_article` workflow against the real Rust VM and five application-owned
objects. Its tests include a mutation followed by failure, and a successful VM
report rejected by a stronger declared application postcondition. This is
separate-process application evidence, not another desktop GUI acceptance.
