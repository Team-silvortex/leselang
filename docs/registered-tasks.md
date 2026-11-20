# Declare Registered GUI Tasks

`defineRegisteredTask` turns common registered-control steps into ordinary
semantic v2 task declarations. It is exported from `@leselang/bridge` and works
with the same Node, Tauri and other registered application hosts. Use it to
describe finding a control, observing its current state, applying an action with
the observed revision, and reading the result again.

The application still chooses the task's meaning, inputs, result and independent
report predicates. Pass the returned declaration to `defineSemanticCatalog`
with the real host's base catalog. That second step validates operation
signatures, references, types and verification contracts; constructing a task
alone does not establish that the host supports it.

## Read And Complete A Task

This complete example declares a `review` button and an unrelated `other`
control as part of its expected application state. It offers two tasks through
the standalone worker's `javascript_task_run` report layout. The read task
preserves the full inventory. Completion requires a pending-to-done transition,
one recorded click, a disabled completed button and an unchanged `other` record.

```ts
import {
  defineRegisteredTask, defineSemanticCatalog,
  type SemanticReportPredicate, type SemanticReportSegment, type SemanticReportTerm,
} from "@leselang/bridge";

type Phase = "objects_before" | "objects_after";
function path(phase: Phase, identifier: string, property?: string): SemanticReportSegment[] {
  const result: SemanticReportSegment[] = [
    { kind: "field", name: phase },
    { kind: "match", field: "identifier", value: identifier },
  ];
  if (property !== undefined) result.push({ kind: "field", name: property });
  return result;
}
const row = (phase: Phase, id: string, property?: string): SemanticReportTerm =>
  ({ kind: "path", path: path(phase, id, property) });
const literal = (value: string | boolean): SemanticReportTerm => ({
  kind: "literal", value: typeof value === "string"
    ? { kind: "string", value } : { kind: "boolean", value },
});
const equal = (id: string, left: SemanticReportTerm, right: SemanticReportTerm): SemanticReportPredicate =>
  ({ id, left, relation: { kind: "equal" }, right });

export function catalog(base: unknown): string {
  const result = { kind: "observed", scalar_type: "string",
    path: path("objects_after", "review", "value") } as const;
  const read = defineRegisteredTask({
    id: "read_status", summary: "Read the review status without changing application state.",
    inputs: [], execution: "no_text", failure: { kind: "string", value: "unavailable" },
    steps: [{ kind: "read_control", control: "review", binding: "observed" }],
    result: { kind: "field", binding: "observed", name: "value" },
    verification: {
      kind: "javascript_task_run", max_fuel: 10_000, require_verified: false, result,
      predicates: [equal("unchanged",
        { kind: "path", path: [{ kind: "field", name: "objects_before" }] },
        { kind: "path", path: [{ kind: "field", name: "objects_after" }] })],
    },
  });
  const complete = defineRegisteredTask({
    id: "complete_review", summary: "Complete the pending review once and preserve the other control.",
    inputs: [], execution: "no_text", failure: { kind: "string", value: "not_completed" },
    steps: [{ kind: "click_control", control: "review", binding: "completed" }],
    result: { kind: "field", binding: "completed", name: "value" },
    verification: {
      kind: "javascript_task_run", max_fuel: 10_000, require_verified: false, result,
      predicates: [
        equal("was_pending", row("objects_before", "review", "value"), literal("pending")),
        equal("is_done", row("objects_after", "review", "value"), literal("done")),
        equal("is_disabled", row("objects_after", "review", "enabled"), literal(false)),
        equal("still_button", row("objects_after", "review", "role"), literal("button")),
        equal("same_title", row("objects_before", "review", "title"), row("objects_after", "review", "title")),
        { id: "one_click", left: row("objects_before", "review", "clicks"),
          relation: { kind: "increased_by", amount: 1 }, right: row("objects_after", "review", "clicks") },
        equal("other_unchanged", row("objects_before", "other"), row("objects_after", "other")),
      ],
    },
  });
  return defineSemanticCatalog(base, {
    version: 2,
    operations: [
      { operation: "gui.find", summary: "Find an exact registered control identifier.", effect: "read", completion: "value" },
      { operation: "gui.observe", summary: "Observe the registered control's current state.", effect: "read", completion: "value" },
      { operation: "gui.click", summary: "Submit a click; observe and verify the application result.", effect: "write", completion: "submission" },
    ],
    tasks: [read, complete],
  });
}
```

This function declares a catalog; it does not create a model or register actual
controls. Supply an application connection with fresh observations for `review`
and `other`, using the [registered application host guide](registered-agent-host.md).
For the Node carrier, pass `catalog` as `createNodeAgentTransport`'s catalog
callback. Its actual worker description supplies the base operation signatures.
An offline provider can select `task read_status` and `task complete_review`;
the same declarations are available to a model provider without a new
application-specific planner branch.

The generated read task performs two VM Effects; completion performs four on
success. `require_verified: false` does not skip verification: the semantic
verifier checks the report and predicates, then the registered host compares
the report with independently read application state.

## Step Vocabulary

| Step | Expansion and result |
| --- | --- |
| `read_control` | Find the exact `control`, observe it and bind that snapshot to `binding`. |
| `set_control_text` | Find and observe; require an enabled, editable text field; submit `text` with its observed revision; require acknowledgement; observe again and require the text-field role and requested value. Bind the final snapshot to `binding`. |
| `click_control` | Find and observe; require an enabled button; submit the click with its observed revision; require acknowledgement; observe again and bind the final snapshot to `binding`. |
| Existing `call` and `guard` | Retain the ordinary semantic vocabulary, including application-specific preconditions and postconditions between these steps. |

Action steps may supply `before: "previous"` to expose their initial snapshot
under a chosen binding. For example, a later ordinary guard can compare
`previous.clicks` with the final snapshot's `clicks`. Omit `before` when the
application does not need that snapshot. Internal object and acknowledgement
bindings are allocated automatically and avoid user definitions and references,
including unknown and forward references; they do not turn those references
into valid task data.

`text` is a normal `SemanticTerm`, so it can use the declared string input, a
literal, or a compatible earlier observation. The task's required `failure`
literal supplies the result of automatically generated guards. It must match
the task result's scalar type. A failed guard stops the generated program at
that point; it does not undo an earlier action.

Post-action observations may show a disabled or read-only control. The
constructor does not demand that an operation remain available after it has
succeeded. A click acknowledgement also does not establish a business result
or imply that `clicks` increased: declare those checks explicitly, as above.

## Validation And Execution Boundaries

`RegisteredTaskDefinition` and `RegisteredTaskStep` provide the public input
types. `defineRegisteredTask` owns its declaration data, rejects accessors and
custom serialization, and returns a deeply frozen `SemanticTaskDescriptionV2`.
Exact unsigned `bigint` literals retain the same authoring rules as
[`defineSemanticCatalog`](semantic-authoring.md).

Limits apply after expansion: at most 21 semantic steps and 16 operation calls,
with zero or one named string input. The final catalog still has its existing
byte, type and predicate bounds. An expansion that exceeds its budget is
rejected rather than truncated. Generated code still passes the ordinary Rust
compiler and uses the host's existing Fuel ceiling; expansion can change the
generated program and its Fuel cost.

The constructor changes no catalog, transport or VM version. It executes no
operation, discovers no GUI object and grants no permission. The original
`poll → authorize → dispatch → accept` flow, revision checks, cancellation and
partial-side-effect semantics remain in the host and VM. No scheduler, retry or
rollback is added. An application that needs a different operation sequence
can keep declaring raw semantic tasks or mix raw steps with these constructors.
