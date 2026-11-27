# Declare JS/TS host operations

A Node application can expose its own commands to an `AgentSession`. The
application declares each operation's Lese name, named parameters, result type,
required capability and JavaScript handler. The agent sees the signatures. The
application chooses the grants, executes the handlers and verifies the outcome.

## Run the independent example

With the repository's [pinned dependencies](../scripts/README.md) installed, run
from the checkout root.

```sh
npm run build --prefix packages/javascript
cargo build --manifest-path hosts/registered/Cargo.toml --locked --offline --profile lean --bin leselang-agent-worker
node consumer/javascript/agent/operations.mjs
```

The [stock Consumer](../consumer/javascript/agent/operations.mjs) submits
`stock.reserve |> stock.remaining` and an item name on the next line. Its offline
provider looks up both operations, checks that the first result fits the second
parameter, and generates this program from the catalog.

```lese
fn main(input: string) =
  bind(previous: stock.reserve(item: input),
       body: stock.remaining(ticket: previous))
```

The item travels separately in `plan.arguments.input`. The VM passes the
reservation ticket to the second operation. The Consumer verifies that the
reservation count grew by one, its item matches the request, and the observed
balance fell by one to the reported `9007199254740998`. A
[translation archive](../consumer/javascript/agent/operations.test.mjs) uses
the same provider with different names and a nullable intermediate result.

## Register operations and connect an agent

`defineOperation` infers TypeScript handler types from the declaration.
`createOperationRegistry` copies the declarations and exposes their immutable
schemas through `describe()`. Handlers stay in the application.

```ts
import { AgentSession, type AgentProvider } from "@leselang/bridge";
import { createNodeOperationHost } from "@leselang/bridge/node";
import { createOperationRegistry, defineOperation } from "@leselang/bridge/operations";

function createCounter(executable: string, provider: AgentProvider, canWrite: () => boolean) {
  let total = 0n;
  const registry = createOperationRegistry([defineOperation({
    name: "counter.increment", parameters: {}, result: "integer", capability: "counter.write",
    handle(_, context) {
      return context.commit(() => ++total);
    },
  })]);
  const host = createNodeOperationHost({
    executable, registry,
    grants: () => canWrite() ? ["counter.write"] : [],
    observe: () => total.toString(),
    verify: ({ before, after, report }) => typeof before === "string" && typeof after === "string"
      && BigInt(after) === BigInt(before) + 1n && report.effects === 1
      && report.result.type === "integer" && report.result.value === after,
  });
  return { session: new AgentSession({ provider, host }), close: () => host.close() };
}
```

Pass the absolute path of the standalone worker as `executable`. During
`session.submit(message)`, the provider receives
`{schema_version: 1, profile: "javascript.operations", operations: [...]}`.
Each entry contains exactly `name`, `parameters`, `result` and `capability`.
Set `profile: "lese.operations"` on `createNodeOperationHost` to use the same
catalog and report identity as the [C# host](dotnet-host.md). The host pins this
choice across discovery, execution and report admission. Omit the option for
the original `javascript.operations` identity.
A counter provider can return `{summary: "Increment the counter", source:
"fn main() = counter.increment()", arguments: {}}`. For several string inputs,
set `plan.inputMode` to `"named_strings"` and pass them by name. Operation
parameters inside the program can use any of the six Lese types below.

| Lese type | JS handler value | Report or wire value |
| --- | --- | --- |
| `integer` | `bigint` from 0 through 2⁶⁴ − 1 | Canonical unsigned decimal string |
| `string` | UTF-8 representable string | JSON string |
| `boolean` | `boolean` | JSON boolean |
| `none` | `null` | JSON null |
| `optional_string` | `string \| null` | JSON string or null |
| `string_list` | `readonly string[]` | JSON string array |

Declare 1–32 unique operations, each with 0–8 required named parameters.
Operation names and capability names are dotted ASCII identifiers of up to
128 characters. Each segment and parameter name is a Lese identifier of up to
64 characters. Each string admits 4096 UTF-8 bytes. A list admits 64 entries
and 4096 UTF-8 bytes in total. Encoded requests and replies also have a 32 KiB
inner wire limit, including JSON escaping and envelope fields. Use
`readOperationValue("integer", report.result.value)` to recover an exact
`bigint` from a report after checking its result type.

## Authority, completion and verification

The Node host pins current grants at admission. The worker compiles the program
against the declared signatures and grants before the first callback. It starts
with 10,000 Fuel and drives each Effect through
`poll → authorize → dispatch → accept`. Each exchange has a scope, a serial,
a challenge and a deadline. Handlers may await work. Use `context.signal` for
cancellable work and `context.commit(() => ...)` for each synchronous state
change after an await. The registry checks live grants at handler entry, each
commit and result delivery. Fuel meters VM work. A handler owns the cost and
cancellation behavior of the application code it invokes.

If switching projects or identities invalidates outstanding work, cancel the
session at that transition. A grant callback reports the current policy each
time it is queried. Two projects can legitimately grant the same capability.

The optional synchronous `observe()` returns a bounded JSON snapshot. The host
copies it immediately before execution and samples it again when verifying the
same submission. `verify({plan, report, before, after, signal})` may return a
boolean or a promise. Check the submitted arguments and the transition in
application-owned state. Without `observe`, both snapshot fields are
`undefined` and the verifier can use its own evidence source. A successful VM
report includes the typed result, accepted Effect count and remaining Fuel.
`AgentSession` publishes `verified` only after clean worker exit and successful
application verification.

A missing required grant prevents the first callback. A handler exception,
invalid reply, cancellation or timeout stops the run. Already committed effects
remain visible and the host does not replay them. An application-verification
failure retains the completed report. Check `outcome.possiblePartialEffects`
and reconcile against application state before submitting a new command.
Call `session.cancel()` to cancel a submission and `host.close()` when disposing
the host.

For a workflow that must resolve a lost reply, supply a business operation
key before the write. The application can record a receipt with the mutation
and expose a read-only lookup for that key. After a failed submission, a
fresh session can use the lookup grant to see whether that specific write
committed. An empty lookup while a writer is still in flight leaves the
outcome open. Reusing the key on an ordinary write can apply it again, as
shown by the [three-Consumer loss experiment](acceptance/agents.md#2026-11-27--losing-a-write-request-or-its-successful-reply).

If a host offers an idempotent write, keep the business key across sessions.
For example, `ledger.add_once(key: key)` increments the balance and records the
resulting receipt on first use. On subsequent calls with that key it returns
the stored receipt, even if other writes have since changed the balance. Keep
the receipt and the business mutation in the same application commit. The
session still needs the write grant to call the operation again. Any other
Effects in the resubmitted program run according to their own contracts.
The [same-key retry experiment](acceptance/agents.md#2026-11-27--same-key-retry-after-an-interrupted-write)
shows the callback counts, commits and original receipt on all three hosts.

The JS registry is framework-independent. The Node adapter selects worker v4.
For registered GUI objects and their semantic task catalogs, use the
[registered agent host](registered-agent-host.md). Run
`sh scripts/check.sh node-consumer` to exercise both paths, including a temporary
application installed from the packed SDK.
