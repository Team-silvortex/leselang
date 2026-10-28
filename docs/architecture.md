# Architecture

## Dependency Direction

Arrows mean consumer to dependency:

```text
leselang-vm -> leselang-hir -> leselang-hir-core
                                     |-> leselang-syntax
                                     |-> leselang-runtime-core
```

The facades also re-export shared syntax and runtime-core types. They do not
implement alternative evaluators or serialize values merely to cross crate
boundaries. The consumer workspace sits outside the package workspace and tests
the same public types with distinct host representations.

`serde` is the only registry dependency of production library code.
`serde_json` supports tests and host-owned checkpoint fixtures. Neither implies
a built-in journal, authentic storage or replay protection.

## Execution

1. Parse source with bounded source input and diagnostics.
2. Resolve the host catalog and compile typed, bounded HIR.
3. Enter a native session with explicit limits and original fuel.
4. Yield a typed effect request; the host checks current authority and dispatches.
5. Accept a corresponding reply, then explicitly poll to resume the session.

The language does not install a global scheduler, timer, worker or global VM lock.
`Vm` is the shared stacked session; `LocalVm` adds local scope ownership around
that same implementation. GUI-local handles remain on their owning thread.
Sharing immutable IR does not authorize concurrent mutation of one session.

## Ownership

| Language foundation | Host or product |
| --- | --- |
| Syntax, typing and bounded computation | Operation names and native implementations |
| Fuel accounting and continuation mechanics | Aggregate ingress, callback work limits and scheduling |
| Reply shape/correlation protocols | Live grants, authentic identity and transport |
| Cooperative local lifetime fencing | Cancellation delivery and effect reconciliation |
| Generic checkpoint and dispatch primitives | Persistence format, atomic settlement and crash recovery |

Leserpent remains a reference host, not a runtime dependency. Its product evaluator,
compatibility facades, SQLite recovery, UI, authentication and orchestration remain
in the product repository. Do not import them to make a native acceptance test pass.

## Acceptance Limits

The existing matrices cover native compilation, bounded control, typed host
effects, suspension/re-entry, cancellation, reply ownership and host-written
checkpoint fixtures. These are not full language/host parity, a production
durable VM, an automatically generated GUI binding, a stable FFI ABI or a security
sandbox. Those milestones must have their own evidence and release decisions.
