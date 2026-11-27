# Scalar application host contract

## The application and the evaluator

A Lese application host publishes operations, grants access to them, runs
their handlers and verifies the result against application state. An evaluator
compiles the submitted Lese source against those declarations and drives each
Effect through `poll → authorize → dispatch → accept`. The host and evaluator
can share a process or exchange messages.

Nuis is the intended primary application host. A Nuis application can implement
the host role using its own types, state and event loop and run the existing
evaluator as a separate executable. Rust embedding, Node and C# currently
exercise the shared semantics. Each new host can use the same fixtures without
importing another host's SDK. The [local object protocol](host-protocol.md)
covers live GUI references, generations and leases separately.

### Discover and run a scalar operation

The application publishes a catalog with `schema_version: 1`, profile
`lese.operations` and an `operations` array. Each declaration contains exactly
`name`, `parameters` (a map of names to scalar types), `result` (a scalar type)
and `capability` (the required grant). Handler code and live grants stay with
the application. Discovery alone grants no authority.

The [shared meter fixture](../fixtures/application-operations/two-effects.json)
declares `meter.add(amount: integer) -> integer` with capability `meter.write`
and `meter.read() -> integer` with capability `meter.read`. The agent can use
that catalog to generate this program.

```lese
fn main() = bind(added: meter.add(amount: 1), body: meter.read())
```

The evaluator checks the program and grants before issuing the first request.
The host dispatches only a declared operation with the declared arguments.
It rechecks live authority and cancellation at handler entry, before committing
a mutation and before delivering the result. Result acceptance lets the
evaluator resume the program. The meter host starts at `9007199254740992`,
observes one increment, then checks the read result against its own balance.
A successful evaluator report and a verified application outcome are separate
observations.

Cancellation, a late or missing reply, revocation or a wrong result can end a
run after an application mutation. The host owns reconciliation. If it offers
same-key retry, it can record a business key and its receipt with the mutation,
then return that receipt on a later authorized call. The [retry fixture](../fixtures/application-operations/lost-reply.json)
covers a still-running application and a settled previous attempt. A business
key can span sessions. A worker request ID and challenge belong to one
exchange.

## Optional stdio binding, worker v4

The current standalone evaluator runs one task per `--serve` process. The
application writes UTF-8 JSON objects terminated by LF to the worker's stdin
and reads the same framing on stdout. Flush each frame, reject duplicate
fields throughout, keep stdin open until the worker exits and drain stderr
separately. The application supplies its scalar catalog. The reference
worker's separate `--describe` command describes its registered-object host.

### Start

Send one start object. This is the full first frame for the shared meter,
with example scope labels.

```json
{"agent_worker_version":4,"kind":"start","profile":"lese.operations","scope":{"window":"meter","epoch":"boot1","session":"run1"},"source":"fn main() = bind(added: meter.add(amount: 1), body: meter.read())","arguments":{},"operations":[{"name":"meter.add","parameters":{"amount":"integer"},"result":"integer","capability":"meter.write"},{"name":"meter.read","parameters":{},"result":"integer","capability":"meter.read"}],"grants":["meter.write","meter.read"]}
```

Each scope label has 1–64 ASCII letters, digits, hyphens or underscores.
Choose a fresh execution scope to fence earlier runs. The worker compiles
entry `main` with 10,000 Fuel. Source admits up to 16 KiB. Entry arguments are
zero to eight named **strings**, up to 4,096 UTF-8 bytes each and 16 KiB total.
Source and entry arguments exclude NUL. Operation parameters and results use
the declared scalar types below.

The catalog has 1–32 operations with unique names and up to eight parameters
each. Operation and capability names contain at least two dot-separated
identifiers and at most 128 ASCII characters in total. Identifiers match
`[A-Za-z_][A-Za-z0-9_]{0,63}`, excluding `fn`, `true`, `false`, `none`,
`body` and the prefix `_lese_`. Grants form a duplicate-free subset of the
declared capabilities. The worker also accepts the existing
`javascript.operations` profile and selects it when the start frame omits
`profile`. The report echoes the selected profile.

| Scalar type | v4 JSON representation |
| --- | --- |
| `integer` | Canonical unsigned decimal **string** from `"0"` through `"18446744073709551615"`, with no sign or redundant leading zeros |
| `string` | JSON string of at most 4,096 UTF-8 bytes |
| `boolean` | `true` or `false` |
| `none` | `null` |
| `optional_string` | A bounded string or `null` |
| `string_list` | Array of up to 64 strings totaling at most 4,096 UTF-8 bytes |

Envelope metadata such as versions, request IDs, budgets, Effect counts and
remaining Fuel uses nonnegative JSON numbers in plain decimal integer syntax.

### Request and reply

Every request or reply envelope contains exactly `agent_worker_version` (4),
`kind`, `challenge` and a nested object named for its kind. For each request
the worker generates a fresh 32-byte challenge encoded as 64 lowercase hex
characters. Only one request may be pending.

| Nested object | Exact fields and handling |
| --- | --- |
| `request` | `transport_version` (1), `window`, `epoch`, `session`, `request_id`, `budget_ms`, `operation`, `arguments`. Check the scope and the next consecutive numeric ID starting at 1. Admit a budget of 1–5,000 ms. Dispatch with the declared parameter names and scalar encodings. |
| `reply` | `transport_version`, `window`, `epoch`, `session`, `request_id`, `status`, `value`, `error`. Echo the correlation fields and the outer challenge. For `ok`, supply the declared scalar value and `error: null`. For `rejected` or `uncertain`, supply `value: null` and a 1–64 character ASCII error code containing letters, digits or underscores. Both stop the run. |

The meter's first request has `operation: "meter.add"` and
`arguments: {"amount":"1"}`. After accepting its successful reply the worker
sends `meter.read` with ID 2, empty arguments and a new challenge. Both reply
values are `"9007199254740993"`. Unsolicited, duplicate, stale and
foreign-scope replies retire the connection.

### Terminal result and bounds

A success frame has exactly `agent_worker_version`, `kind: "report"` and
`report_json`. Parse the last field as a **string containing JSON**. The
meter's decoded report is shown below.

```json
{"schema_version":1,"kind":"operation_task_run","profile":"lese.operations","result":{"type":"integer","value":"9007199254740993"},"effects":2,"fuel_remaining":9994}
```

Check the result's tag and value, selected profile, accepted Effect count
against successful replies, and remaining Fuel in the range 0–10,000. Accept
the report only after stdout ends without trailing data and the worker exits
successfully. An error frame has exactly `agent_worker_version`,
`kind: "error"` and a diagnostic `error` string of at most 1,024 UTF-8 bytes.
Failure can also surface as EOF, timeout or nonzero exit. Stop dispatching
and retain evidence of any application commits.

The reference binding allows up to 256 requests. It bounds the start frame
at 128 KiB, request and reply frames at 64 KiB and their inner encoded
objects at 32 KiB. The report text is bounded at 128 KiB and its outer frame
at 512 KiB. Allow space for LF in each frame. The reference worker waits up
to five seconds for the start and sets a five-second deadline per exchange.

Run the wire conformance checks from the repository root against an
independently supplied worker executable.

```sh
LESE_WORKER=/absolute/path/to/worker node --test consumer/javascript/agent/operations-wire.test.mjs
```

The probe uses Node's standard library. To run from a separate directory,
copy `operations-wire.test.mjs` and `two-effects.json` there, point
`LESE_OPERATIONS_FIXTURE` at that JSON file and supply the evaluator in
`LESE_WORKER`. It checks the granted two-Effect exchange and verifies that
a read-only grant stops the program before either meter callback.
