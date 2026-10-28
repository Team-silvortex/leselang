# Embed A Host

Start with the complete [native counter](../crates/leselang-vm/examples/native_counter.rs).
It imports `leselang_vm::native`, implements the host contracts directly, and
uses the real compiler and session engine. There is no separate example interpreter.

The example defines a typed `counter.add(amount: integer)` operation, compiles a
helper/capture program, executes native mutations only after current authority
checks, and resumes through corresponding typed receipts. Two calls produce 42.

## Run The Boundaries

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

## Adapter Responsibilities

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

## Dependency Setup

The [consumer manifest](../consumer/Cargo.toml) is a working local-path example.
Use `leselang-vm` as the native entry point. No product feature must be disabled
in this repository: all supported feature combinations remain native.

After this repository has an actual commit and remote, consumers should pin an
immutable Git revision or a deliberately published package version. Do not use
an invented URL/revision or treat the initial 0.1.0 package version as evidence
of a published crate or guaranteed stable ABI.
