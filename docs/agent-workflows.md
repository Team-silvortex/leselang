# Sequential Agent Workflows

An application can accept one request that selects several declared semantic
tasks, such as inspecting controls, saving supplied text and checking the final
application state. `AgentWorkflowSession` admits the complete plan before the
first execution, then executes and verifies each step in order through the
existing `AgentHost` interface.

```text
One user request + current host catalog
  → one provider plan containing 1–8 declared tasks
  → validate every task, argument and generated program
  → for each step:
      fresh catalog description → unchanged contract → execute → verify
  → ordered step reports and a verified, failed or cancelled outcome
```

This is an additive SDK interface. `AgentSession`, `AgentProvider`,
`createSemanticProvider` and `createModelProvider` retain their single-task
contracts. The Rust compiler, VM and task carrier do not gain a scheduler or a
new execution mode. Each workflow step uses the ordinary explicit
`poll → authorize → dispatch → pump → accept` path.

## Construct A Workflow Session

The public `@leselang/bridge` entry exports `AgentWorkflowSession`,
`AgentWorkflowProvider`, `AgentWorkflowPlan`, `AgentWorkflowState` and
`AgentWorkflowOutcome`. A workflow provider receives the same `AgentContext`
as a single-task provider and returns `{ steps: WorkflowPlan[] }`:

```ts
import {
  AgentWorkflowSession, prepareSemanticCatalog,
  type AgentHost, type AgentWorkflowProvider,
} from "@leselang/bridge";

function createSaveWorkflow(host: AgentHost) {
  const provider: AgentWorkflowProvider = {
    async plan({ message, catalog }, signal) {
      if (signal.aborted) throw new Error("cancelled");
      const declared = prepareSemanticCatalog(catalog);
      return {
        steps: [
          declared.prepare("inspect_controls", {}).plan,
          declared.prepare("save_message", { message }).plan,
          declared.prepare("check_controls", {}).plan,
        ],
      };
    },
  };
  return new AgentWorkflowSession({ provider, host });
}

const session = createSaveWorkflow(applicationHost);
const outcome = await session.submit("周五完成设计评审");
```

This deterministic example treats the submitted message as literal text to save.
It uses task identifiers from the Tauri reference catalog. A general application
chooses its own provider and declarations; the SDK has no built-in form profile.
Each step requires a declared task identifier, zero or one exactly named string
argument, and the source produced by that catalog's `prepare` method. The SDK
owns the admitted plan and uses host-authored task summaries.

Before any `host.execute` call, the session validates all steps against the
initial semantic catalog. An empty plan, a ninth step, an unknown task, a wrong
argument or changed source anywhere in the plan rejects the entire plan. A
valid first step cannot run while an invalid last step remains unexamined.
Admission does not grant execution permission: the host must still compile,
authorize and independently verify each program.

For an existing single-task provider, adapt its result explicitly:

```ts
const provider: AgentWorkflowProvider = {
  async plan(context, signal) {
    return { steps: [await singleTaskProvider.plan(context, signal)] };
  },
};
```

This wrapper is suitable when the provider already returns a semantic task with
its declared identifier and canonical source. The workflow session requires
semantic admission even though the older general `AgentSession` also accepts
providers without a task identifier.

## Model Planning

`createModelWorkflowProvider({ complete })` uses the same application-owned
`ModelRequest` transport as [`createModelProvider`](model-provider.md). Its
response contract is different: the model must return one JSON object containing
an ordered `steps` array, or a refusal object.

```json
{"steps":[{"task":"inspect_controls","arguments":{}},{"task":"save_message","arguments":{"message":"周五完成设计评审"}},{"task":"check_controls","arguments":{}}]}
```

```json
{"error":"The requested work cannot be completed using the declared tasks."}
```

```ts
import { AgentWorkflowSession, createModelWorkflowProvider } from "@leselang/bridge";

const session = new AgentWorkflowSession({
  host: applicationHost,
  provider: createModelWorkflowProvider({ complete: applicationModelTransport }),
});
const outcome = await session.submit(
  "先检查应用控件，再保存“周五完成设计评审”，最后核对应用状态。",
);
```

The model selects only existing tasks and literal arguments. The SDK generates
each LeseLang program from its declaration. Source code, permissions,
verification claims, result references and extra fields are rejected. The whole
model response is validated before it becomes a workflow plan. The original
`createModelProvider` still accepts `{task, arguments}` and rejects the workflow
response form; callers choose the desired contract explicitly.

The reference makes one model request per submission, with the user message,
task summaries and full host catalog. It does not ask the model to replan after
each step or include prior conversation turns. This version has no branches,
loops or substitution of an earlier result into a later argument. A plan that
needs runtime-dependent arguments requires a different application integration.

## Fresh Catalogs And Step Verification

The workflow pins its initial complete catalog, including verification metadata.
Before each step, the host is described again and the description must match
that pin. A changed verification predicate can stop execution even when the
generated Lese source would be identical. The comparison does not accept source
equality as evidence that task meaning stayed the same.

Use stable original JSON text for catalogs. Raw JSON is pinned exactly, so even
a formatting-only change can stop a pending workflow. This conservative rule
also preserves exact integer tokens and prevents a changed contract from being
silently adopted halfway through execution. A catalog change after completed
steps leaves their outcomes available and stops the remaining work.
For plain-object catalogs, the SDK owns and freezes the description and compares
its JSON representation, including property order. Switching between raw JSON
and an equivalent object also changes the pin. Return one stable representation
throughout a workflow.

Each step uses the existing `AgentSession` discipline: describe, plan admission,
execute and application verification must settle before the next step starts.
The Tauri adapter checks the corresponding report and fresh registered-control
observations. A successful Promise, a provider summary or a non-error VM result
does not replace application verification.

A verified workflow means that every step's declared postconditions held when
that step completed. It does not mean that all earlier postconditions still hold
after later steps. For example, two successful saves can deliberately replace
the same field. Add a final declared verification task when the workflow needs
a final-state check. The workflow is not an atomic transaction.

## Failure, Cancellation And Evidence

`submit` returns an immutable aggregate outcome with the admitted plan, ordered
step outcomes and an overall `verified`, `failed` or `cancelled` status. Only
completed attempts appear in the step outcomes; the plan retains the unattempted
remainder. The state observer reports progress while the submission is active.

`AgentWorkflowState` adds `totalSteps`, `completedSteps` and an optional zero-based
`stepIndex` to the ordinary task state. `completedSteps` counts verified steps,
not merely dispatched ones; `totalSteps` is zero until plan admission succeeds.
An unsuccessful `AgentWorkflowOutcome` may include `stoppedStep`, the zero-based
attempted or next step at which progress stopped. Its `steps` array includes a
failed or cancelled attempt when one exists. The aggregate `taskId` identifies
the submission; each child outcome retains the single-task session's own id.

If execution or verification fails, later steps do not start. If earlier steps
entered execution, the aggregate outcome retains `possiblePartialEffects: true`
on failure or cancellation, even when those earlier steps verified successfully.
This flag is conservative: entering a read-only task also counts as entering
execution. The retained reports explain what was observed; the flag never
promises rollback or proves that a specific mutation occurred.

Cancellation reaches the currently awaited provider or host operation. The
workflow remains busy until that operation actually settles. A second submission
cannot overlap it, and a late plan or step report cannot start another step.
Cancellation between verified steps prevents the next execution. Observer
callbacks cannot grant authority or make an unverified step successful.

There is no automatic retry, rollback, compensation or resume-from-step action.
Resubmitting a request is a new workflow against current application state and
may repeat earlier side effects. An application should use the retained evidence
when deciding what follow-up work is appropriate.

## Bounds And Runtime Ownership

| Boundary | Limit or ownership |
| --- | --- |
| Steps per submission | 1–8 |
| User message / initial catalog | 8 KiB / 256 KiB UTF-8 |
| Each task argument / source | 4,096 bytes / 16 KiB UTF-8 |
| Each step report | 128 KiB UTF-8; at most eight retained reports |
| Complete model selection response | 16 KiB UTF-8 shared by all steps |
| Model HTTP request | Existing native 45-second deadline and response bounds |
| VM Fuel and authorization | Fresh ordinary host session for each step |

The model response's combined limit still applies when all individual arguments
fit their own limits. An eight-step plan cannot carry eight maximum-size string
arguments through a 16 KiB model reply. Direct application providers remain
subject to the workflow plan and per-step admission limits.

The Tauri reference keeps the original 10,000-Fuel budget for each task session.
A three-step workflow creates three independently authorized VM sessions; it
does not combine their Fuel or carry object handles, receipts or grants from one
step into the next. Application state and the page lifetime do persist.
The page's existing 64 execution-session limit counts steps, not user submissions.
Exhaustion can stop a workflow after earlier steps have executed, and the
aggregate failure retains that evidence. Model-request identities have their
own existing 64-request page bound.

## Tauri Conversation Entry

The Tauri model mode uses `createModelWorkflowProvider` and displays progress
and typed results for each step in the same conversation entry. Its existing
offline commands are adapted as one-step workflows. Switching modes is explicit
and disabled during a submission; failures do not switch providers or retry.
Model service configuration remains in the Rust process, as described in the
[model provider guide](model-provider.md#configure-the-tauri-transport).

SDK and application tests can validate sequencing, all-step admission,
cancel/late-result fencing, catalog changes and retained evidence without a paid
model call. The explicit loopback protocol fixture can supply predetermined
multi-step selections to the real WebView. Those checks establish transport and
application execution behavior; they do not establish live-model interpretation
quality. A genuine model service needs separate natural-language acceptance.

To exercise the bundled protocol fixture, follow the
[model-service setup](model-provider.md#verification-scope) with model name
`protocol-workflow-fixture-not-a-model`. These additional exact inputs exercise
sequence admission:

| Submission | Expected observation |
| --- | --- |
| `先检查控件，再保存“多步骤工作流 ✓”，最后核对控件。` | Three ordered results: `integer: 3`, `string: "saved"`, `none`; one Save and 15 VM Effects |
| `测试后续步骤无效` | A valid save followed by an undeclared task is rejected before the first execution; no Save or other new Effect |

The first result assumes a successful run from a fresh reference application.
The fixture only recognizes its documented exact inputs and returns predetermined
selections. Its legacy model id remains available for testing the older
single-task provider response shape.

### Recorded macOS WebView Check

On 2026-11-20, the actual bundled agent WebView used the explicitly labelled
workflow protocol fixture. One HTTP planning request produced the three-step
inspection/save/check sequence. The conversation showed three verified results
in order: `integer: 3`, `string: "saved"`, and `none`. Both the input and saved
value contained `多步骤工作流 ✓`, with one Save and zero Other-control clicks.

The invalid-second-step fixture was rejected before execution, retaining that
same application state. A delayed planning request was cancelled; the fixture
observed its connection close without completing the response. A subsequent
model-selected inspection succeeded, followed by an explicit switch to offline
mode and a successful `task check_controls` command.

The service recorded four model requests in total. Native tracing recorded
exactly 21 authorized and accepted Effects: 15 for the initial three steps,
three for the subsequent inspection, and three for the offline check. Invalid
planning and cancellation added no application Effects, and offline execution
made no model request. Mid-execution failure and cancellation between completed
steps are covered separately by SDK and application-host integration tests;
the recorded desktop run did not inject those faults. This is real GUI/VM
acceptance with predetermined planning replies, not a live-model inference test.
