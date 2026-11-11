# Architecture

## Host Direction

LeseLang is an agent control language whose first host target is GUI control;
CLI adapters share the same observation, action and verification contracts.
The language foundation owns typed execution and generic local protocol
primitives. A host owns the objects, live permissions, event processing and the
meaning of submission or completion. See the
[local host protocol draft](host-protocol.md) for the current contract and its
acceptance gates.

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

The `leselang` executable lives in `leselang-vm` and consumes its public
`embedding` API as a reference host. It supplies `std.print`, file/stdin ingress,
CLI permissions and diagnostics. `std.print` is a host registration, not a new
language primitive; the library remains usable without this executable.

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

Native hosts can opt into typed native operands through
`CallFlowEnvironment::call_native_argument_type`. The default refuses them.
The compiler keeps these operands in ordinary `Call` argument trees and retains
their `PureType::Result` metadata, so the existing structural checks, lexical
scope and helper hygiene remain in force. Runtime `PreparedValueCall` preparation
reuses the pure evaluator and original Fuel while requiring native adapter
validation. Scalar `PreparedCall` and the high-level scalar embedding API remain
available with their existing contracts.

## Ownership

| Language foundation | Host or product |
| --- | --- |
| Syntax, typing and bounded computation | Operation names and native implementations |
| Fuel accounting and continuation mechanics | Aggregate ingress, callback work limits and scheduling |
| Reply shape/correlation protocols | Live grants, authentic identity and transport |
| Cooperative local lifetime fencing | Cancellation delivery and effect reconciliation |
| Opaque local references, generations and revision comparison | Native object storage, stable snapshots and recording external state changes |
| Generic checkpoint and dispatch primitives | Persistence format, atomic settlement and crash recovery |

Leserpent remains a reference host, not a runtime dependency. Its product evaluator,
compatibility facades, SQLite recovery, UI, authentication and orchestration remain
in the product repository. Do not import them to make a native acceptance test pass.

## Acceptance Limits

The existing matrices cover native compilation, bounded control, typed host
effects, suspension/re-entry, cancellation, reply ownership and host-written
checkpoint fixtures. Local protocol conformance uses CLI and headless GUI
models with explicit queues and event pumping, distinguishing an action's
submission from its observed postcondition. The source discovers one of multiple
native objects, binds its typed result and passes it to observation/action calls.
This tests actual dynamic identity rather than a fixed object hidden behind an
operation key. Native helper parameter annotations and native-valued field
projections remain outside this milestone; scalar embedding remains a separate
convenience profile.

A concrete GUI framework adapter, remote/durable host protocol, full
language/host parity and stable FFI ABI each require their own evidence and
release decisions. The current tests do not establish those milestones or a
security sandbox for native callbacks.
