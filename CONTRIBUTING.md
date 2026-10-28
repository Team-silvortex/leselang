# Contributing

Keep the language foundation independent of host products. New operations belong
in a host adapter unless they are genuine language or generic protocol primitives.
Do not add a GUI toolkit, account service, product DTO or persistence policy to
the language crates. Existing native APIs and tests are the executable contract;
historical extraction notes do not override current code.

## Verification

Fetch the pinned dependencies once, then validate both workspaces offline:

```sh
cargo fetch --locked
cargo fetch --manifest-path consumer/Cargo.toml --locked
cargo fmt --all -- --check
cargo fmt --manifest-path consumer/Cargo.toml --all -- --check
cargo test --workspace --all-features --locked --offline --profile lean
cargo clippy --workspace --all-features --all-targets --locked --offline --profile lean -- -D warnings
cargo test --manifest-path consumer/Cargo.toml --locked --offline --profile lean
cargo clippy --manifest-path consumer/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
cargo test --example native_counter --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean -- --queued
```

The consumer is deliberately a separate Cargo workspace. Its repository-boundary
test checks actual Cargo metadata, including all-features builds, so a sibling
checkout or optional product dependency cannot silently become a prerequisite.

Use the `lean` profile for local checks. Root and consumer commands share `target/`
through `.cargo/config.toml`; avoid assigning a new target directory per test.
No containers or external server are needed for these native tests.

## Changes And Releases

Keep changes in the owning crate, add a focused regression, and validate the
separate consumer whenever public types, features or ownership change. Test
partial effects, stale replies and cancellation as well as the success path.
Never present a source budget as preemption of arbitrary native callbacks.

The independent version line starts at 0.1.0. Update all five crate manifests and
the separate consumer together; the repository-boundary test rejects version drift.
Package numbering is separate from operation, protocol and checkpoint schema versions.
Both lockfiles are tracked. Update them intentionally when packages or dependencies change.
Keep MIT metadata and the LICENSE notice. Crates are not yet registry-publishable:
release preparation must choose version policy, versioned inter-crate dependencies,
package contents and a real repository URL before removing `publish = false`.

The product repository retains a compatibility baseline during handoff. Do not
develop separate native implementations in both repositories; record deliberate
backports until the product consumes an immutable Leselang revision. See
[the migration checklist](docs/migration.md).
