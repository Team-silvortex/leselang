# Leselang Runtime Core

Host-neutral scalar, lifecycle, accounting and protocol foundations for Leselang.
This crate is not a parser, complete evaluator, GUI adapter or durable queue.
Its only production registry dependency is `serde`; `serde_json` is test-only.

## Responsibilities

- Typed scalar values, bounded collections and scalar operation semantics.
- Fuel accounting, checked clock arithmetic and bounded retry/admission decisions.
- Native operation catalogs, parameter binding and request/reply contracts.
- Local execution scope ownership and cooperative cancellation observations.
- Generic capture, dispatch, checkpoint and result ownership primitives.

Opaque host inputs do not gain unconditional Clone, Send, Sync or serialization
requirements. GUI-local values can remain local. Native callbacks and destructors
are trusted; core limits do not preempt them or roll back their mutations.

`Admission` is bounded pre-admission work, not an accepted-effect journal.
`LocalExecutionScope` revokes cooperatively on close or Drop, not on every process
abort. Serialized observations do not authenticate receipts, grant execution
authority or prove crash-consistent settlement. Hosts own those policies.

Use [the native VM facade](../leselang-vm/src/native.rs) for a complete embedding
entry point, or these primitives directly when implementing a host protocol.
The [architecture guide](../../docs/architecture.md) describes the layer boundary;
the [counter example](../leselang-vm/examples/native_counter.rs) is executable.

## Checks

```sh
cargo test -p leselang-runtime-core --locked --offline --profile lean
cargo clippy -p leselang-runtime-core --all-targets --locked --offline --profile lean -- -D warnings
```

Older product-specific acceptance notes are preserved as
[historical provenance](../../docs/provenance/runtime-core-baseline.md), not a
requirement to check out the product repository.
