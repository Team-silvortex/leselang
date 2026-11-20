# Sequential Agent Workflows

An application can accept one request that selects several declared semantic
tasks, such as inspecting controls, saving supplied text and checking the final
application state. A task may also use a verified earlier result as its string
input through an explicit result binding, or require that result to equal an
expected value before continuing. `AgentWorkflowSession` admits the complete
plan before the first execution, then executes and verifies each step in order
through the existing `AgentHost` interface.

```text
One user request + current host catalog
  → one provider plan containing 1–8 declared tasks
  → validate every task, argument, requirement and generated program
  → for each step:
      fresh catalog description → unchanged contract
      → check any verified-result requirement → resolve verified inputs
      → execute → verify, or stop on failure
  → ordered step reports and a verified, failed or cancelled outcome
```

This is an additive SDK interface. `AgentSession`, `AgentProvider`,
`createSemanticProvider` and `createModelProvider` retain their single-task
contracts. Workflow sequencing does not add a Rust or VM scheduler. Each
workflow step uses the ordinary explicit
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
Each step requires a declared task identifier, exactly its declared string
arguments, and the source produced by that catalog's `prepare` method. Semantic
v1/v2 retain zero or one input; v3 supports zero to eight independently named
strings. Every v3 plan carries `inputMode: "named_strings"`, including no-input
tasks, and admission rejects a missing or mismatched marker. The SDK owns the
admitted plan and uses host-authored task summaries. See the
[named string input guide](named-task-inputs.md) for v3 declarations and the
complete multi-field reference application.

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

## Bind A Verified Result To A Later Input

`prepareSemanticWorkflow(rawCatalog, steps)` prepares a complete workflow from
declarative steps and returns an `AgentWorkflowPlan`. A step argument can be a
literal string or a `WorkflowResultReference` with a zero-based `fromStep` and
an explicit `format`. The helper validates the graph against the host's existing
semantic declarations; it does not change the catalog schema or task signatures.

```ts
import {
  AgentWorkflowSession, prepareSemanticWorkflow,
  type AgentWorkflowProvider,
} from "@leselang/bridge";

const provider: AgentWorkflowProvider = {
  async plan({ catalog }, signal) {
    if (signal.aborted) throw new Error("cancelled");
    return prepareSemanticWorkflow(catalog, [
      { task: "inspect_controls", arguments: {} },
      { task: "save_message", arguments: {
        message: { fromStep: 0, format: "decimal" },
      } },
      { task: "check_controls", arguments: {} },
    ]);
  },
};
const session = new AgentWorkflowSession({ provider, host: applicationHost });
const outcome = await session.submit("先检查控件数量，把这个数量保存为文本，再核对控件。");
```

The helper takes the original catalog value, such as `AgentContext.catalog`,
not the object returned by `prepareSemanticCatalog`. Its public step type is
`SemanticWorkflowStep`; the inline reference type is `WorkflowResultReference`.
The example reads the actual accepted integer result from step 0, converts it
to decimal text and supplies that text to step 1's `message` argument.

Binding source types can come from a fixed result contract in semantic v1, v2
or v3, or from the explicit `scalar_type` of a v2/v3 observed-result contract.
An [observed result](host-semantics.md#version-2-observed-results) is verified
against a separate actual observation in the same execution report. This allows
the result to vary with application state without replacing verification with
a type-only check.

Only these explicit conversions are supported:

| Source result type | Required format | Target string |
| --- | --- | --- |
| `string` | `"string"` | The exact returned text |
| `integer` | `"decimal"` | Exact unsigned decimal digits |
| `boolean` | `"boolean"` | `"true"` or `"false"` |

There is no implicit coercion. `none`, `optional_string`, `string_list`, native
objects and snapshots cannot be used as binding sources. Decimal conversion
preserves the full `u64` value, including `18446744073709551615`; it does not pass
through JavaScript `Number`. A resolved value must still fit the ordinary
4,096-byte string input bound and pass text validation. A v3 step rechecks its
whole resolved argument map against the 16 KiB combined value bound before
execution.

Every reference must point to an earlier step, and each target argument can
have at most one binding. A forward reference, self-reference, unknown argument,
type/format mismatch or conflicting literal rejects the complete plan before
any execution. These restrictions make cycles impossible. Several later steps
may refer to the same earlier result. Bindings cannot select arbitrary report
fields or carry native authority across sessions.

For a v3 task, different named inputs may bind different earlier results in the
same step. For example, `publish_article` can receive
`title: { fromStep: 0, format: "string" }` and
`content: { fromStep: 1, format: "string" }`. Each source must independently pass
verification before its value becomes available. This uses the same binding
descriptors and sequencing rules; the target's values remain entry arguments
rather than text substituted into generated source.

For a result that is referenced later, both its catalog-prepared semantic
verifier and `host.verify` (or the optional `host.verifyDetailed`) must accept
the source step. Only then can the later step obtain its value from that step's
actual accepted `report.result`. The
catalog's expected result, a provider-supplied value, a summary or an unverified
report cannot stand in for the execution result. Failed, cancelled and late
source results never feed a target step. Cancellation and the pinned-catalog
check remain in force before the target executes.

For example, a host can declare a no-input `read_message` task whose string
result must equal the observed current input value. A later task can save that
verified text without the provider predicting it:

```ts
const plan = prepareSemanticWorkflow(rawV2Catalog, [
  { task: "read_message", arguments: {} },
  { task: "save_message", arguments: {
    message: { fromStep: 0, format: "string" },
  } },
  { task: "check_controls", arguments: {} },
]);
```

The same catalog can handle different current input strings. Each run still
requires equality between the typed result and its declared observed value,
the original report predicates, and the application's fresh-state verification.
The binding reads the accepted `result.value` after those checks; it does not
read arbitrary report fields or reuse a previous run's value. Source reading
and a later save are separate verified sessions, not an atomic read/write pair.

The admitted plan represents the data dependency rather than predicting its
value. `prepareSemanticWorkflow` generates the target's canonical source with
an empty string argument placeholder and adds a frozen descriptor to optional
`AgentWorkflowPlan.bindings`:

```ts
const binding: AgentWorkflowBinding = {
  step: 1, argument: "message", fromStep: 0, format: "decimal",
};
```

This descriptor's public type is `AgentWorkflowBinding`. Applications that
construct descriptors directly must use that empty placeholder; supplying a
nonempty literal and a binding for the same target is rejected. The session
revalidates all descriptors and templates during admission. Immediately before
the target starts, it rebuilds the task plan with the resolved literal argument;
the Lese source does not interpolate the returned text.

`outcome.plan` retains the immutable templates and symbolic bindings.
`outcome.steps[i].plan` records the concrete plan actually attempted, including
the resolved string argument. If all inputs are literal, the helper omits
`bindings` and existing `{steps}` providers continue to work unchanged.

## Require A Verified Result Before Continuing

A `SemanticWorkflowStep` may include one optional `requires` value of type
`WorkflowResultRequirement`. It names an earlier step, an explicit scalar
format and the exact expected text. For example, read the current application
text and save it only if the verified read equals the requested value:

```ts
const plan = prepareSemanticWorkflow(rawV2Catalog, [
  { task: "read_message", arguments: {} },
  {
    task: "save_message",
    arguments: { message: { fromStep: 0, format: "string" } },
    requires: { fromStep: 0, format: "string", equals: "条件执行回归 ✓" },
  },
  { task: "check_controls", arguments: {} },
]);
```

The requirement and result binding are independent. A target may instead have
a literal argument or no argument. The requirement's source must be an earlier
step with a declared string, integer or Boolean result in semantic v1, v2 or v3.
Its format uses the same explicit conversion as a binding. `equals` is always
a string: string expectations preserve exact text up to 4,096 UTF-8 bytes with
no NUL; decimal expectations must be canonical unsigned digits in the `u64`
range, with no leading zero except `"0"`; Boolean expectations are exactly
`"true"` or `"false"`. There is no coercion, expression, substring test or
arbitrary report path.

The helper normalizes each inline requirement into a frozen
`AgentWorkflowRequirement` in optional `plan.requirements`, adding the target
`step` index. Admission revalidates these descriptors even when a provider
constructs them directly. At most one requirement may target each step, with
at most eight descriptors in a plan. Unknown fields, forward/self references,
duplicate targets, unsupported result types, format mismatches or invalid
expected values reject the complete plan before any execution. An earlier
requirement that would fail does not excuse an invalid later step.

A requirement source must pass both its catalog's semantic report verifier
and the host's selected Boolean or detailed verifier, even when no result binding
uses that source. Before the target executes, its fresh catalog must still match
the pinned description.
The session then converts the source's actual accepted `report.result` and
compares it exactly with `equals`. A mismatch produces
`workflow_requirement_not_met` and stops the workflow before the target's
`host.execute`; subsequent steps remain unstarted. Earlier outcomes and effects
are retained, including the conservative `possiblePartialEffects` flag.
Failure is not treated as a skipped or successful step.

Optional `outcome.requirements` contains only comparisons that actually took
place. Each immutable `AgentWorkflowRequirementOutcome` retains the descriptor
plus `actual` and `satisfied`. An unvisited, cancelled or invalid comparison
does not receive a fabricated `false` entry. The aggregate plan preserves all
admitted requirements, while `stoppedStep` identifies where progress ended.

This is a condition on an earlier verified result, not a lock on application
state. A GUI can change after that observation and before a later action.
Live authorization, revision checks and fresh application verification still
belong to the host. Use a host-declared operation when the application requires
an atomic compare-and-update; workflow requirements do not provide one.

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

By default, the model selects only existing tasks and literal arguments. The SDK
generates each LeseLang program from its declaration. Source code, permissions,
verification claims, result references, requirements and extra fields are
rejected. The whole model response is validated before it becomes a workflow plan. The original
`createModelProvider` still accepts `{task, arguments}` and rejects the workflow
response form; callers choose the desired contract explicitly.

An application can enable the bounded result-reference vocabulary using
`ModelWorkflowProviderOptions.resultBindings`. It defaults to `false`; the
single-task provider has no result-binding mode.

```ts
const provider = createModelWorkflowProvider({
  complete: applicationModelTransport,
  resultBindings: true,
});
```

When enabled, the workflow model may return an inline reference in a task's
argument map:

```json
{"steps":[{"task":"inspect_controls","arguments":{}},{"task":"save_message","arguments":{"message":{"fromStep":0,"format":"decimal"}}},{"task":"check_controls","arguments":{}}]}
```

The provider uses `prepareSemanticWorkflow` to validate and normalize that
response into canonical templates and binding descriptors. The model does not
return a `bindings` field or choose an arbitrary value path. Unsupported formats,
wrong source types and references supplied while the option is disabled fail
before execution. This option permits a declared data dependency; it does not
permit a model to predict or manufacture the referenced result.

Independently enable `ModelWorkflowProviderOptions.resultRequirements` to allow
an optional `requires` object on a model-selected step. It also defaults to
`false`; enabling it does not enable result bindings. The Tauri conversation
explicitly enables both options:

```ts
const provider = createModelWorkflowProvider({
  complete: applicationModelTransport,
  resultBindings: true,
  resultRequirements: true,
});
```

The model uses the same `{fromStep, format, equals}` shape shown above. It cannot
supply normalized top-level `requirements`, actual comparison results or
verification claims. The SDK validates the whole plan and derives the actual
comparison from verified execution evidence. Expected text expresses the
requested condition; it cannot replace the observed result or its verifier.

The reference makes one model request per submission, with the user message,
task summaries and full host catalog. It does not ask the model to replan after
each step or include prior conversation turns. This version has no branches,
loops, arbitrary transformations or expression evaluation between steps.
Runtime-dependent arguments use only the explicit verified-result conversions
described above.

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
For bound inputs, inspect the attempted step's concrete plan to see the resolved
argument; the aggregate plan intentionally retains its symbolic dependency.

With the optional `AgentHost.verifyDetailed` hook, a failed step can include a
bounded `verificationFailure`; the aggregate outcome and final failed state
retain the same diagnostic. Successful and cancelled outcomes do not carry it.
Legacy Boolean hosts and existing error strings remain unchanged. See
[verification diagnostics](verification-diagnostics.md) for cause codes and
report correlation versus fresh application-state checks. Diagnostics explain
failure; they do not authorize repair or make a failed step usable as input.

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
| Inputs per task | v1/v2: zero or one string; v3: zero to eight named strings |
| Each task argument / source | 4,096 bytes / 16 KiB UTF-8 |
| Combined argument values per v3 task | 16 KiB UTF-8, checked again after result bindings resolve |
| Result bindings | One per target string argument, referencing only an earlier step |
| Result requirements | One per target step, at most eight; only strict equality to an earlier verified result |
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

The explicit v3 plan marker selects worker protocol v2 in the Node transport
and `agent_start_named_task` in Tauri. Rust hosts use `prepare_named_task` with
the existing prepared-task driver. All parameters are compiled and bound before
application requests. Legacy plans retain worker v1, `prepare_task` and the
original Tauri command's zero-or-one-string admission. Outer worker versions
are pinned for the lifetime of each child; unsupported versions fail without
fallback. This routing preserves the inner Bridge protocol, Fuel, live grants,
cancellation and partial-effect behavior.

## Tauri Conversation Entry

The Tauri model mode uses `createModelWorkflowProvider` with `resultBindings: true`
and `resultRequirements: true`, and displays progress and typed results for each
step in the same conversation entry. Its existing
offline commands are adapted as one-step workflows. Switching modes is explicit
and disabled during a submission; failures do not switch providers or retry.
Model service configuration remains in the Rust process, as described in the
[model provider guide](model-provider.md#configure-the-tauri-transport).
The conversation marks steps that use an earlier verified result. The execution
details retain each attempted step's concrete argument map beside its original
raw report, so a bound value can be inspected without rounding the report's
integer evidence.
It also displays required and actual values when a result condition is evaluated.
A failed requirement is shown as stopped work, with no claim that the target
operation ran or that earlier effects were undone.

The bundled form and board catalogs remain v2, so their existing offline
commands retain literal text after the colon. When an application supplies a
v3 catalog, the same conversation entry displays named-input commands such as
`task publish_article:{"title":"文本","content":"文本"}`; a no-input command
remains `task read_title`. Only v3 interprets that argument text as a JSON map.

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
| `先检查控件数量，把这个数量保存为文本，再核对控件。` | Verify the count, save its resolved decimal text `3`, then check controls; one additional Save and 15 VM Effects |
| `测试结果引用类型错误` | Integer-to-string format mismatch rejects the whole plan before execution |
| `测试引用尚未完成的步骤` | A reference to a later step rejects the whole plan before execution |
| `读取当前输入框的文本，原样保存，再核对控件。` | With the v2 catalog, read the current string, bind it into Save, then check controls; one additional Save and 14 VM Effects |
| `读取当前文本，只有它等于“条件执行回归 ✓”时才原样保存，再核对控件。` | With matching input, three verified steps and one Save (14 VM Effects); with different input, the read verifies and the requirement stops execution before Save (two VM Effects) |
| `测试条件结果类型错误` | A requirement with the wrong declared result format rejects the complete plan before execution |

The first result assumes a successful run from a fresh reference application.
The fixture only recognizes its documented exact inputs and returns predetermined
selections. Its legacy model id remains available for testing the older
single-task provider response shape.

For the observed-result case, place different strings in the application input
and submit the same fixture request for each one. The expected result is that
each run reads and saves its own current input, using the same catalog and plan
shape. The fixture supplies only the task selection and reference; it does not
supply the text to save. Inspect the source report, the resolved save argument,
saved output and unaffected controls for each run. This is an expected acceptance
procedure; a fixture reply alone does not establish that the GUI passed it.

### Recorded macOS WebView Check

The dated evidence is preserved in [Sequential Workflows](acceptance/agents.md#sequential-workflows).

### Recorded Verified-Result Binding Check

The dated evidence is preserved in [Verified Result Bindings](acceptance/agents.md#verified-result-bindings).

### Recorded Dynamic Observation Check

The dated evidence is preserved in [Dynamic Observation Results](acceptance/agents.md#dynamic-observation-results).
