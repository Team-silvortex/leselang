# Host-Declared Operation Semantics And Task Verification

An operation signature answers which values a host accepts. The optional
`semantics` catalog extension describes what an operation means, which finite
task the host offers, and what evidence establishes the task's result. The
independent `catalog_agent` and JavaScript SDK consume these declarations without
adding an application name, operation name or control identifier to their
planners. Consumers must explicitly support the semantic version they accept.

This is host-authored metadata. It is not a permission grant, an authenticated
claim about another process, or a way to infer arbitrary application behavior.
The selected executable remains trusted host code. Its implementation must
enforce identity, revision, scope and current authority when executing Effects.

## Preserve The Existing Catalog

`CatalogDescription` and `OperationDescription` keep their existing fields.
`CatalogDescription::to_json()` keeps its original schema-version 1 output.
The existing version-1 API remains available unchanged:

```rust,ignore
use leselang_runtime_core::discovery::semantics::SemanticDescription;

let catalog = /* description projected from actual operation registration */;
let semantics: SemanticDescription = /* application-owned task declarations */;
let json = catalog.to_json_with_semantics(&semantics)?;
```

This method validates the existing catalog and the semantic references,
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

Version 2 is a separate opt-in description and serializer. It retains the base
catalog schema and finite task vocabulary while adding a result contract that
compares the returned value with declared observation evidence. Fixed result
contracts retain their existing JSON shape in both semantic versions. Existing
v1 types, `to_json_with_semantics`, catalogs and byte-for-byte goldens remain
unchanged; a host does not acquire v2 behavior merely by upgrading the SDK.
Only `semantics.version` changes. The base catalog schema, execution report
schema and transport protocol retain their existing versions.

```rust,ignore
use leselang_runtime_core::discovery::semantics::{self, v2};

let legacy: semantics::SemanticDescription = /* existing fixed declarations */;
let upgraded = v2::SemanticDescription::from(legacy);
let json = catalog.to_json_with_semantics_v2(&upgraded)?;
```

`semantics::v2` provides `SemanticDescription`, `TaskDescription`, `ReportContract`
and `ResultContract`. The three description/contract structs implement conversion
from their v1 counterparts; fixed results become `ResultContract::Exact` and
keep their old wire shape without an `exact` wrapper. `ResultContract` also
converts from `ScalarValue`. Conversion selects `semantics.version: 2` but does
not establish validity: declarations still require validation against the real
catalog. `to_json_with_semantics_v2` performs that validation and preserves the
same complete-output bound.

## Describe Meaning And Completion

JavaScript and TypeScript hosts can use the public
[`defineSemanticCatalog`](semantic-authoring.md) authoring interface. It checks
readonly v1/v2 declarations against the actual base catalog and writes exact
integer JSON, without changing this vocabulary or execution semantics.

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
[`hosts/shared/semantics.rs`](../hosts/shared/semantics.rs), outside the
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
Both versions' literals support integer, Boolean, string and `none`. Optional strings
and string lists can be observed and used in conditions, but cannot yet be
supplied as declaration literals.

The finite task vocabulary supports equality, string-list membership, an unsigned integer
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
`--host`, literal `--host-arg` selectors, task and task input. Both versions accept
zero inputs or one named string input, passed through `--text` as data.

## Declare Independent Verification

The verification contract states a report kind, typed result contract,
Fuel ceiling, whether the host must report `verified: true`, and named
predicates over the report. The agent always checks the report's schema and
catalog profile, typed result, Fuel and the number of calls in the declared
task before checking those predicates.

Version 1 requires the existing fixed scalar result, for example
`{"kind":"integer","value":3}`. Version 2 also accepts that unchanged fixed
shape. A fixed result is still compared exactly; declaring v2 does not loosen
it into a type-only expectation.

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

## Version 2 Observed Results

A task can already return a declared scalar field from an observed snapshot.
With a fixed result contract, the expected value must be known in advance.
The v2 `observed` contract instead requires the returned value to equal a
separate declared observation in the actual execution report:

```json
{
  "kind": "observed",
  "scalar_type": "string",
  "path": [
    { "kind": "field", "name": "native_after" },
    { "kind": "match", "field": "identifier", "value": "input" },
    { "kind": "field", "name": "value" }
  ]
}
```

This object is the task's `verification.result`. The task's own `result` remains
an ordinary typed term, such as the `value` field of the snapshot returned by
`gui.observe`. The host must declare matching types for that result term and
the observed result contract. No new LeseLang expression, branching form or
runtime type is introduced.

In Rust, use `v2::ResultContract::Observed { scalar_type, path }`. Its
`scalar_type()` method reports the declaration's type, not evidence that an
execution returned that type. The admitted observed `scalar_type` is exactly
`string`, `integer` or `boolean`. `none`, optional strings, string lists and
native values are not observed-result kinds.

The path uses the existing `ReportSegment` vocabulary and contains one to eight
segments. Its first segment must be a field other than `result`; a path into the
returned result itself cannot establish independent observation evidence.
Field names and match-field names are nonempty and bounded to 128 UTF-8 bytes;
match values retain the 4,096-byte string bound. Empty paths, unknown fields in
the contract, unsupported types and invalid selectors are rejected when
admitting the catalog. Runtime path lookup still requires every field and
exactly one object for each array match.

Verification checks all of the following:

1. The report has the required schema, profile and kind, valid Fuel and expected
   Effect count, and any required host `verified` flag.
2. The report's typed `result` has the declared scalar type, and its value equals
   the actual value selected by the observed path. Integers are compared without
   passing through JavaScript `Number`; missing or differently typed evidence
   fails rather than being coerced.
3. Every original named report predicate passes. An observed result does not
   remove before/after state, count, role or unrelated-control checks.
4. The application independently checks fresh state and binds the report to the
   execution being verified. The Tauri adapter retains this separate check.

The catalog supplies a path, not the value that should appear at that path.
For a `read_message` task, one run may return `first note` and another `second
note` using the same catalog, but each returned string must match that run's
observed input. A report containing the right scalar type with the wrong value
is rejected. Matching two fabricated report fields is also insufficient to
replace the host's independent fresh-state check.

The JavaScript `prepareSemanticCatalog` API and independent Rust catalog agent
dispatch explicitly on semantic version 1 or 2. They reject `observed` under v1
and keep fixed v2 comparisons strict. Source generation uses the same finite
task terms and existing compiler limits.
[Workflow result bindings](agent-workflows.md#bind-a-verified-result-to-a-later-input)
use `scalar_type` to admit an observed source's explicit conversion. They still
extract the value from the accepted typed result only after both semantic and
application verification; the observation path is not a general binding path.

## Bounds And Compatibility

| Semantic item | Version 1 and 2 limit |
| --- | --- |
| Operation descriptions | 64, each referring to an actual operation |
| Tasks | 1–8, with unique identifiers |
| Steps per task | 1–21, including at most 16 calls |
| Guard conditions | 1–8 |
| Report predicates | 1–64, with unique identifiers |
| Report path segments | 1–8 |
| Observed result type (v2 only) | `string`, `integer` or `boolean`; first path segment is a non-`result` field |
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

The original four-task Tauri catalog remains available through `describe_agent`,
`--describe-agent`, or `agent_describe` with `taskCatalog: true`. It retains its
v1 bytes, fixed result contracts and original golden fixture.

The conversation opts into a separate five-task v2 catalog through
`describe_agent_observed`, `--describe-agent-observed`, or `agent_describe` with
both `taskCatalog: true` and `observedResults: true`. Its additional no-input
`read_message` task finds the registered input, observes it and returns its
current value. Its observed-result contract matches that string to the actual
input observation and retains the unchanged-state predicates. Reading uses two
VM Effects and no mutations. The existing tasks retain their fixed contracts
within the v2 catalog. The separate fixtures are
[`tauri-agent-semantics-v2.json`](../fixtures/host-semantics/tauri-agent-semantics-v2.json)
and [`tauri-read-message.lese`](../fixtures/host-semantics/tauri-read-message.lese).

Both catalogs' `tauri_task_run` evidence has
no business `verified` flag: the host's declared predicates and fresh frontend
observations establish success. The original one-shot catalog and `tauri_run`
report remain unchanged. See the [task carrier contract](agent-entry.md#semantic-planning-and-host-verification)
for input and result bounds.

See the [catalog schema](host-discovery.md) for the base description and the
[agent workflow](agent-workflow.md) for commands, saved artifacts and desktop
acceptance. Fuel, permission revocation, cancellation, late replies and partial
side effects retain their existing runtime behavior. A failed or interrupted
execution is never automatically replayed or rolled back.
