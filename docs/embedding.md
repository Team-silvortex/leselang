# Embed LeseLang In Rust

Use `leselang_vm::embedding` to register scalar host operations, compile source,
and drive a local session with explicit Effect handoffs. This optional API is
selected by importing the module; it requires no Cargo feature, background
worker, async executor, or product repository. It adapts the existing HIR compiler
and `LocalVm`, keeping their evaluation, fuel and lifetime rules.

The complete [embedded counter](../crates/leselang-vm/examples/embedded_counter.rs)
registers `counter.add(amount: integer)`, compiles a helper/capture program and
returns **42** after two host Effects:

```sh
cargo run --example embedded_counter --locked --offline --profile lean
cargo run --manifest-path consumer/Cargo.toml --example embedded_counter --locked --offline --profile lean
```

## Register, Compile And Start

`HostBuilder::register` takes an operation name, named parameter declarations,
one scalar result type, a required capability label, and a Rust callback.
`parameter(name, scalar_type)` declares a required parameter. Use
`NamedParameter::optional(String, ScalarTypeSet)` for an optional argument;
omission supplies no value or implicit default. The callback can read arguments
by name through `Arguments::get`.

Operation names use dotted ASCII identifier segments and are bounded to 128
bytes total; parameter names are ASCII identifiers bounded to 64 bytes. Lexical
keywords and unqualified built-in/control names are reserved, so an operation
such as `counter.add` cannot be silently replaced by the language's `add`.

Operation names use ASCII identifier segments separated by dots, up to 128
bytes in total; parameter names are single identifiers up to 64 bytes. Lexer
keywords and unqualified language builtins/control names are rejected at
registration. Prefer names such as `counter.add` to make host ownership clear.

```rust
use leselang_vm::embedding::{
    Fuel, HostBuilder, HostFailure, ScalarType, ScalarValue, parameter,
};

let total = std::cell::Cell::new(0_u64);
let mut builder = HostBuilder::new();
builder.register(
    "counter.add",
    vec![parameter("amount", ScalarType::Integer)],
    ScalarType::Integer,
    "counter.write",
    |arguments, fuel| {
        fuel.charge(1)
            .map_err(|_| HostFailure::Failed("host fuel exhausted".into()))?;
        let Some(ScalarValue::Integer(amount)) = arguments.get("amount") else {
            return Err(HostFailure::Failed("missing integer amount".into()));
        };
        let next = total.get().checked_add(*amount)
            .ok_or_else(|| HostFailure::Failed("counter overflow".into()))?;
        total.set(next);
        Ok(ScalarValue::Integer(next))
    },
)?;
let host = builder.build();
let grants = ["counter.write"];
let program = host.compile(
    "fn main() = bind(first: counter.add(amount: 19), body: counter.add(amount: 23))",
    &grants,
)?;
let mut session = host.start(&program, &grants, Fuel::new(1_000))?;
```

Callbacks may borrow local state and use `Cell` or `RefCell`; they need no
`Send`, `Sync`, or `'static` bound. Keep resource-specific policy checks inside
the callback immediately before mutation. Capability labels are host-provided
permissions, not grants that a script can manufacture.

The built host owns immutable operation metadata. Compilation validates `main`
and its helpers, including cold branches, before creating an owned `Program`.
A program is tied to the host that compiled it. Start multiple sessions from
that program to reuse IR; each receives an independent scope and Fuel meter.
Callback state captured by the same host is still shared, so use separate hosts
when native state should also be isolated.

Call `host.describe()` to inspect the immutable registered operation names,
parameter domains, result types, required capabilities and local protocol
version. Discovery invokes no callbacks and grants no permissions. For native
object references, dynamic object operands, historical observations and
revision-checked action attempts, see the
[local host protocol draft](host-protocol.md). That native HIR opt-in facility
does not add object values to this scalar convenience layer.

## Drive Every Effect Explicitly

The caller owns the loop and chooses when each transition occurs:

```rust
use leselang_vm::embedding::Poll;

loop {
    match session.poll()? {
        Poll::Request(request) => {
            let authorized = session.authorize(request)?;
            let receipt = session.dispatch(authorized)?;
            session.accept(receipt)?;
        }
        Poll::Value(value) => break value,
        Poll::Awaiting => {
            // Return control to your event loop until its queued handoff is ready.
            // A synchronous loop should always retain and handle the request.
            return Err("pending host work has not been delivered".into());
        }
        Poll::Terminal(end) => return Err(format!("execution ended: {end:?}").into()),
    }
}
```

| Step | What happens |
| --- | --- |
| `poll` | Runs one language slice and yields a request, a final value, `Awaiting`, or a terminal status. It never invokes an operation callback. |
| `authorize` | Checks the request against this session and current grants, returning an opaque authorized request. |
| `dispatch` | Rechecks current authority and invokes the callback under the existing `LocalVm` lifecycle fence with its original Fuel. It returns a receipt without accepting it. |
| `accept` | Checks receipt identity, live authority, result type and value bounds. Success prepares re-entry; only the next `poll` runs the continuation. |

Requests, authorized requests and receipts cannot be cloned or forged through
this API. Keep them in a host-owned local queue when work must wait between
steps. Calling `poll` while a receipt is outstanding returns `Awaiting`; it does
not replay the callback or recharge the pending slice. The adapter provides no
external queue, worker scheduling, durable receipt serialization or retry loop.

## Revocation, Cancellation And Failure

Use `session.set_grants(&[])` to revoke capabilities for that session. Grants
are checked during execution, authorization, dispatch, receipt acceptance and
continuation re-entry. A token authorized before revocation cannot dispatch
under the revoked policy. If authority is revoked after `accept`, the next
`poll` refuses re-entry without running the next Effect.

`session.cancel()` closes the local scope and prevents further language or
callback work. Completion, terminal failure and dropping the session also close
its lease. Cancellation after dispatch rejects a late receipt while preserving
the mutation already committed by the callback. Deliver cancellation to any
external queue or resource yourself; the VM cannot cancel arbitrary native work
or undo it.

An ordinary failed acceptance returns `Rejected<Receipt>` with the original
receipt in `input`, preserving its ownership and the pending session. This
does not authorize another dispatch. A host may explicitly reconsider a receipt
after a policy change, or cancel the session. Invalid results retain their
original value for inspection through `Receipt::value()`.

Callback failures are classified explicitly:

| Callback result | Session effect |
| --- | --- |
| `Ok(value)` | Produces a receipt; the session still awaits acceptance. |
| `Err(HostFailure::Failed(message))` | Ends the session as `Failed`. |
| `Err(HostFailure::Uncertain(message))` | Ends the session as `HostUncertain`; the host must reconcile any uncertain external effect. |
| `Err(HostFailure::Cancelled)` | Ends the session as `Cancelled`. |
| Escaping Rust panic | The existing lifecycle fence closes the scope as `HostUncertain`, then the panic continues unwinding. |

None of these outcomes roll back earlier mutations. A callback that mutates
state and then returns `Failed` retains that partial effect. Check conditions
before mutation where possible, and choose `Uncertain` when delivery cannot be
determined. There is no automatic replay after a failure or rejected receipt.

## Budgets And Supported Values

`HostBuilder::new()` uses bounded defaults. Customize them with
`HostBuilder::with_limits(Limits { ..Limits::default() })`:

- `max_source_bytes` limits input before parsing.
- `catalog` bounds operation and parameter counts.
- `compilation` controls source, expanded IR, helper and compilation work limits.
- `execution` controls evaluation shape, bindings and stack frames.

Supply the initial `Fuel` when starting a session. Every language slice, capture,
re-entry and explicitly charged callback uses that same meter. There is no
implicit refill across Effects. A callback should charge before bounded work and
classify exhaustion as a failure. Native Rust remains trusted: Fuel cannot
preempt a callback that blocks or loops, and mutable meter access must not be
used to replace or refill it accidentally.

This convenience layer accepts scalar argument domains and one declared scalar
return type per operation. It validates the existing scalar string/list bounds
on replies. It does not adapt native object fields, groups, custom result
domains, durable recovery, wire protocols or GUI dispatch. Use
`leselang_vm::native` and the existing `OperationCatalog`, HIR environments and
`LocalVm` for those custom adapters; their interfaces remain available.

Native adapters can opt into typed result operands using
`CallFlowEnvironment::call_native_argument_type`, whose default refuses them,
and prepare mixed arguments with `PreparedValueCall` /
`evaluate_value_call_arguments_in_scope`. The reference `protocol_hosts` program
discovers an object, binds the native reply and passes it to observation and
action operations. Actual object/lease validation and live authority stay with
the adapter; the shared compiler and VM still own language execution. See the
[native object operand contract](host-protocol.md#native-object-operands) for
the remaining helper and projection limits.

The high-level failure suite runs both inside the library workspace and from
the independent consumer:

```sh
cargo test -p leselang-vm --test embedding --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --test embedding --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --locked --offline --profile lean
```

## Lower-Level Reference Host

The complete [native counter](../crates/leselang-vm/examples/native_counter.rs)
imports `leselang_vm::native` and implements the host contracts directly. It
uses the same real compiler and session engine, with synchronous and explicit
queued host loops. Two calls produce 42.

```sh
cargo run --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean -- --cancel
cargo run --example native_counter --locked --offline --profile lean -- --queued
cargo run --example native_counter --locked --offline --profile lean -- --queued-cancel-pending
cargo run --example native_counter --locked --offline --profile lean -- --queued-cancel-reply
```

Cancellation after the first synchronous effect retains 19. Cancelling queued
work before dispatch produces no mutation; cancelling after dispatch rejects the
late reply but retains the committed mutation. The other queued VM still reaches
42. `--deny`, `--revoke` and `--queued-revoke` deliberately exit unsuccessfully.

The example's tests cover these refusals, native failure, cold invalid input,
fuel cutoffs, stale/wrong-typed replies and isolated cancellation:

```sh
cargo test --example native_counter --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --example native_counter --locked --offline --profile lean
```

## Custom Adapter Responsibilities

Supply an operation catalog with signatures, capability requirements and stable
schema identity. Bind source names against that catalog before execution. Keep
live permission checks immediately before native mutation, not only at compile
time. Validate authentic receipts against the current execution and request.

Bound source size, compilation, IR, frame depth, fuel and native callback work.
Fuel only bounds work charged through the language protocols. It cannot stop a
callback that blocks or loops forever, and replenishing a meter is host policy.

Preserve the original session and fuel across yields. Explicitly close the local
scope and deliver cancellation to any external queues or collectors. A rejected
reply is not automatically a retry authorization; a committed effect is not
rolled back by cancellation or by a failed later expression.

For a GUI host, map each operation onto the framework's dispatcher and keep
thread-affine objects local. A GUI event loop may drive polling, but Leselang
does not automatically discover widgets or adapt a toolkit. The headless GUI
consumer tests are protocol examples, not a shipping Avalonia/FFI integration.
The separate [AppKit host](appkit-host.md) demonstrates the next concrete step:
real application-owned macOS buttons and a text field, bounded control
discovery, text replacement, native target/action dispatch, typed object operands
and an explicit main-thread event pump. Its form verifies the edited value and
the value captured by Save, with input changes invalidating stale Save actions.
Its framework bridge does not alter the scalar convenience API or become a
language dependency.

The [macOS Accessibility host](macos-ax-host.md) adapts the same native contracts
to an explicitly configured TextEdit document in another process. It retains
local typed references, checks the process/document/element scope and live
permission, then explicitly pumps a queued text replacement. A new observation
verifies the resulting value. External AX snapshots cannot supply complete
mutation history or an atomic read/write transaction, so this adapter documents
its weaker freshness guarantee separately. The scalar convenience API and
the application-owned AppKit host keep their existing contracts.

## Dependency Setup

The [consumer manifest](../consumer/Cargo.toml) is a working local-path example.
Use `leselang-vm` as the entry point, choosing `embedding` for the scalar
convenience API or `native` for the complete host contracts. A local consumer
needs only the VM crate for the high-level API:

```toml
[dependencies]
leselang-vm = { path = "../leselang/crates/leselang-vm" }
```

Adjust the path to your checkout. No product feature must be disabled in this
repository: all supported feature combinations remain native.

After this repository has an actual commit and remote, consumers should pin an
immutable Git revision or a deliberately published package version. Do not use
an invented URL/revision or treat the initial 0.1.0 package version as evidence
of a published crate or guaranteed stable ABI.
