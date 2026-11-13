# Host-Declared Operation Semantics And Task Verification

An operation signature answers which values a host accepts. The optional
`semantics` catalog extension describes what an operation means, which finite
task the host offers, and what evidence establishes the task's result. The
independent `catalog_agent` can consume these declarations without adding an
application name, operation name or control identifier to its planner.

This is host-authored metadata. It is not a permission grant, an authenticated
claim about another process, or a way to infer arbitrary application behavior.
The selected executable remains trusted host code. Its implementation must
enforce identity, revision, scope and current authority when executing Effects.

## Preserve The Existing Catalog

`CatalogDescription` and `OperationDescription` keep their existing fields.
`CatalogDescription::to_json()` keeps its original schema-version 1 output.
Hosts opt in using:

```rust,ignore
use leselang_runtime_core::discovery::semantics::SemanticDescription;

let catalog = /* description projected from actual operation registration */;
let semantics: SemanticDescription = /* application-owned task declarations */;
let json = catalog.to_json_with_semantics(&semantics)?;
```

The new method validates the existing catalog and the semantic references,
then appends a top-level `semantics` object with its own `version: 1`.
The complete output remains at most 256 KiB, including its trailing LF.
No backend, permission query, session, callback or GUI operation is needed.
The metadata types can be deserialized as unchecked data; callers must bound
input and validate them against the actual catalog before using them.

Old consumers that tolerate unknown catalog fields can continue reading the
type catalog. The reference agent keeps its original strategies when
`semantics` is absent. A present but null, malformed, unknown-version or
unsupported semantic declaration fails planning; it never silently falls
back to an old application-specific strategy.

## Describe Meaning And Completion

Each described operation names an actual entry in `operations` and supplies:

| Field | Meaning |
| --- | --- |
| `operation` | The exact registered operation name. |
| `summary` | A bounded human-readable description of its meaning. |
| `effect` | `read` or `write`, as classified by the host. |
| `completion` | `value` for a returned value, or `submission` for an acknowledgement that still needs observation. |

These labels do not override `required_capability` or determine whether an
operation is safe to retry. The reference agent never retries an execution.
In particular, `submission` is not evidence that the requested application
state has been reached. A task should follow it with observation and guards.

The descriptions for the reference applications live in
[`hosts/reference_semantics.rs`](../hosts/reference_semantics.rs), outside the
language crates. AppKit, Tauri and Calculator publish them through their JSON
discovery entry points. The generic JavaScript registered-controls catalog
does not claim that the reference form's controls or task exist.

## Declare A Finite Task

A task contains an identifier, summary, optional string input, ordered steps,
result and verification contract. It uses two step kinds:

- `call` binds the result of a declared operation, supplying named arguments.
- `guard` requires all declared conditions; failure returns the supplied scalar
  immediately, preventing later steps from running.

Arguments and conditions can refer to a literal, the task input, an earlier
binding, or a declared scalar field of an earlier native snapshot. Forward
references, duplicate bindings, unknown fields and incompatible argument
types are rejected before source generation. Native objects remain native
references; metadata cannot manufacture an object handle or compare objects
as scalar values.
Version 1 literals support integer, Boolean, string and `none`. Optional strings
and string lists can be observed and used in conditions, but cannot yet be
supplied as declaration literals.

Version 1 supports equality, string-list membership, an unsigned integer
increase, and optional-string equality. Optional-string equality checks
presence first, so absent text never equals an empty string. There are no
loops, branches that select alternative actions, retry rules, source snippets,
shell commands or arbitrary expressions in this declaration format.

The reference generator turns the finite steps into ordinary LeseLang helper
functions, using only live variables at each helper boundary. Guards become
normal `choose` expressions. The host's existing parser, typed compiler,
function/parameter limits and VM Fuel remain authoritative. Generated source
can have a different Fuel cost from a handwritten program; the original host
budget is never raised to accommodate it.

The task's `execution` field chooses one of three existing reference-host
argument conventions: `named_text`, `text` or `no_text`. It cannot select an
executable or supply arbitrary process arguments. The user still supplies
`--host`, literal `--host-arg` selectors, task and task input. Version 1 accepts
zero inputs or one named string input, passed through `--text` as data.

## Declare Independent Verification

The verification contract states a report kind, expected typed scalar result,
Fuel ceiling, whether the host must report `verified: true`, and named
predicates over the report. The agent always checks the report's schema and
catalog profile, typed result, Fuel and the number of calls in the declared
task before checking those predicates.

Report paths contain only explicit field names and unique string-key matches
within arrays. A match must identify exactly one object; a missing or duplicate
match fails. There is no JSONPath evaluator or executable predicate code.
Operands may read a path, require a particular scalar type at a path, take an
array's length, use the task input, or use a scalar literal. Comparisons support
equality, a checked unsigned integer increase, and an unsigned integer upper
bound. Reference form declarations retain the original `i64::MAX` ceiling for
reported click counts even though the language's integer type is `u64`.

For example, a saved-value predicate can read a uniquely identified record and
compare it with the task's input:

```json
{
  "id": "saved_value",
  "left": {
    "kind": "scalar",
    "path": [
      { "kind": "field", "name": "native_after" },
      { "kind": "match", "field": "identifier", "value": "save" },
      { "kind": "field", "name": "value" }
    ],
    "scalar_type": "string"
  },
  "relation": { "kind": "equal" },
  "right": { "kind": "input", "name": "text" }
}
```

Use separate assertions for collection size, role, enabled state, intended
value, callback count and unaffected controls. A Calculator task additionally
checks the last expression: the number `3` alone does not establish `1 + 2`.
An optional string path must exist even when its value is `null`. Structural
equality preserves array order; use unique named matches and a length check
when the application treats an array as an unordered set of controls.

Report data is bounded to 128 KiB and duplicate JSON keys are rejected. Missing
fields, wrong scalar types, ambiguous selectors, integer overflow or any failed
named predicate prevent success. Native instance continuity remains inside
the host; public scalar evidence does not replace those checks or make a GUI
read/write sequence atomic.

## Bounds And Compatibility

| Semantic item | Version 1 limit |
| --- | --- |
| Operation descriptions | 64, each referring to an actual operation |
| Tasks | 1–8, with unique identifiers |
| Steps per task | 1–21, including at most 16 calls |
| Guard conditions | 1–8 |
| Report predicates | 1–64, with unique identifiers |
| Report path segments | 1–8 |
| Task inputs | Zero or one string input |
| Summary | 1–1,024 UTF-8 bytes |
| Scalar string | At most 4,096 UTF-8 bytes |
| Declared report `max_fuel` | 1–1,000,000; a verification ceiling, never a runtime budget grant |
| Generated source | At most 16 KiB, within existing host compiler limits |

Semantic metadata is consistency-checked data, not a proof that its author
chose sufficient postconditions. Application authors own the meaning of their
operations and the strength of their verification contract. A provider can
read summaries and tasks to help select an intent, but the current reference
agent selects a declared task explicitly and makes no external model call.
The JavaScript SDK also exposes `prepareSemanticCatalog` and
`createSemanticProvider`; the [in-app conversation entry](agent-entry.md)
uses the same declaration vocabulary. Its generator is checked against the
Rust source fixtures. Original JSON text preserves exact integer tokens and
duplicate-key checks; already-rounded JavaScript numbers are rejected.

The Tauri conversation entry requests its task catalog explicitly
(`agent_describe` with `taskCatalog: true`, or offline `--describe-agent`). That
catalog covers no-input checks and a differently named string input, with
Boolean, string, integer and `none` results. Its `tauri_task_run` evidence has
no business `verified` flag: the host's declared predicates and fresh frontend
observations establish success. The original one-shot catalog and `tauri_run`
report remain unchanged. See the [task carrier contract](agent-entry.md#semantic-planning-and-host-verification)
for input and result bounds.

See the [catalog schema](host-discovery.md) for the base description and the
[agent workflow](agent-workflow.md) for commands, saved artifacts and desktop
acceptance. Fuel, permission revocation, cancellation, late replies and partial
side effects retain their existing runtime behavior. A failed or interrupted
execution is never automatically replayed or rolled back.
