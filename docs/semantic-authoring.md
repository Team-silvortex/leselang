# Author Host Semantics In JavaScript And TypeScript

`defineSemanticCatalog(base, semantics)` attaches application-owned task
declarations to an actual host operation catalog and returns validated raw JSON.
It is exported from the browser-compatible `@leselang/bridge` root entry, along
with readonly TypeScript types for the existing semantic v1 and v2 vocabulary.
It works with Node, Tauri or another registered application transport.

The application declares what operations mean and which evidence establishes a
task's result. This helper checks and serializes those declarations. It does not
infer a workflow, choose success conditions, execute an Effect or grant authority.
The compiler, VM and independent application verifier retain their existing jobs.

For registered GUI controls, [`defineRegisteredTask`](registered-tasks.md) can
construct the common find/observe/action/observe sequences before this catalog
validation step. Applications still supply the task's business result and report
predicates, and may retain or mix the ordinary call/guard declarations below.

## Declare A Task Against The Real Catalog

This complete v2 declaration reads a registered `draft` object's actual value.
Its result must agree with the final observation, and the full inventory must
remain unchanged. It can be used with the standalone worker and the publication
application from the [Node host guide](node-agent-host.md).

```ts
import { defineSemanticCatalog, type SemanticDescription } from "@leselang/bridge";

const declaration = {
  version: 2,
  operations: [
    { operation: "gui.find", summary: "Find an exact registered identifier.",
      effect: "read", completion: "value" },
    { operation: "gui.observe", summary: "Read the object's current state.",
      effect: "read", completion: "value" },
  ],
  tasks: [{
    id: "read_draft", summary: "Read the draft without changing application state.",
    inputs: [], execution: "no_text",
    steps: [
      { kind: "call", binding: "target", operation: "gui.find", arguments: [
        { name: "name", value: { kind: "literal", value: { kind: "string", value: "draft" } } },
      ] },
      { kind: "call", binding: "observed", operation: "gui.observe", arguments: [
        { name: "target", value: { kind: "binding", name: "target" } },
      ] },
    ],
    result: { kind: "field", binding: "observed", name: "value" },
    verification: {
      kind: "javascript_task_run", max_fuel: 10_000, require_verified: false,
      result: { kind: "observed", scalar_type: "string", path: [
        { kind: "field", name: "objects_after" },
        { kind: "match", field: "identifier", value: "draft" },
        { kind: "field", name: "value" },
      ] },
      predicates: [{
        id: "inventory_unchanged",
        left: { kind: "path", path: [{ kind: "field", name: "objects_before" }] },
        relation: { kind: "equal" },
        right: { kind: "path", path: [{ kind: "field", name: "objects_after" }] },
      }],
    },
  }],
} satisfies SemanticDescription;

export function describeApplication(baseRaw: string): string {
  return defineSemanticCatalog(baseRaw, declaration);
}
```

Pass `describeApplication` as the `catalog` callback of
`createNodeAgentTransport`. The callback receives the real worker's offline
base description. `createRegisteredAgentHost` consumes the resulting catalog;
the existing offline or model provider selects from its tasks without an
application-specific planner branch. The helper does not discover application
objects: the application must register `draft` and supply independent observations.

TypeScript catches declaration shape errors. `defineSemanticCatalog` then checks
operation names, argument domains, native fields, input conventions, prior
bindings, result contracts and predicate types against the supplied base.
An unknown operation, forward binding, wrong type or unsupported version throws
before any catalog is returned. Rust still compiles the generated LeseLang
before execution; TypeScript types do not replace that check.

## Versions And Public Types

| Public types | Purpose |
| --- | --- |
| `SemanticDescription`, `SemanticDescriptionV1`, `SemanticDescriptionV2` | Complete operation semantics and task declarations |
| `SemanticTaskDescriptionV1`, `SemanticTaskDescriptionV2`, `SemanticTaskInput` | A task's inputs, steps, result and report contract |
| `SemanticOperationSemantics` | Operation meaning, read/write classification and completion meaning |
| `SemanticTaskStep`, `SemanticTaskArgument`, `SemanticTerm`, `SemanticCondition` | The existing finite call/guard vocabulary |
| `SemanticReportContractV1`, `SemanticReportContractV2`, `SemanticObservedResult` | Fixed or independently observed result contracts |
| `SemanticReportPredicate`, `SemanticReportTerm`, `SemanticReportRelation`, `SemanticReportSegment` | Named comparisons over explicit evidence paths |
| `SemanticLiteral`, `SemanticInteger`, `SemanticScalarType` | Supported literal and scalar domains |

Version 1 accepts fixed results. An observed result requires `version: 2`, both
in the TypeScript declaration and at runtime. Version 2 also accepts the existing
fixed result shapes. The helper does not upgrade versions automatically, and
rejects any base that already has a `semantics` field, including `null`. Compose
the desired declaration before calling it instead of replacing an admitted
catalog's task contract.

The wire vocabulary and all limits remain those of the
[host semantic contract](host-semantics.md). The complete output is at most
256 KiB, with at most 65,536 JSON nodes and depth 32. Existing smaller task,
predicate, text and Fuel bounds still apply.

## Exact Integers And Owned Data

An unsigned integer in an authored declaration may be a safe JavaScript Number
or an exact `bigint`, for example:

```ts
import type { SemanticLiteral } from "@leselang/bridge";

const maximum = {
  kind: "integer", value: 18_446_744_073_709_551_615n,
} satisfies SemanticLiteral;
```

`defineSemanticCatalog` writes that value as the exact JSON integer token
`18446744073709551615`. Pass the returned raw string to the provider/host; do not
round-trip it through `JSON.parse` and `JSON.stringify`. Unsafe Number values
are rejected rather than converted back into a guessed integer. Negative or
out-of-range integers fail, and a large integer does not enlarge the Fuel limit.

This bigint support is specific to author declarations. Base catalogs keep the
existing input contract: original raw JSON preserves duplicate keys and integer
tokens for validation, while already-decoded objects must use safe Numbers.
`prepareSemanticCatalog` and report verification retain their existing contracts;
ordinary object ingress does not gain implicit bigint support. Raw version
tokens such as `1.0` or `1e0` cannot be normalized into valid integer versions.

Inputs are copied without calling getters or `toJSON`. Cycles, accessor fields,
symbol keys, array holes and non-JSON prototypes are rejected. The caller's
objects are not mutated or frozen; later edits cannot change the returned
catalog string. Application-owned declarations still need the usual trusted
host boundary and independent verification of real state.

## Plain JavaScript Applications

The same declarations can be checked using JSDoc and `checkJs`:

```js
/** @type {import("@leselang/bridge").SemanticDescriptionV2} */
const declaration = { /* your version: 2 declaration */ };
```

The Node reference shares its publication task between old v1 and current v2
entries through [`publicationSemantics`](../consumer/javascript/semantic-contract.mjs).
Its [v2 catalog](../consumer/javascript/agent/catalog.mjs) declares four tasks
using the registered task constructor and public writer; the original v1
declaration remains available unchanged. The author modules are checked by
`npm run check:catalogs --prefix consumer/javascript`, also included in that
Consumer's test command. The external packed Consumer repeats these checks
against installed public declarations without Node typings.
