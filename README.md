# Leselang

An embeddable agent control language with GUI control as its first host target
and support for CLI hosts. LeseLang separates observing an object, submitting an
authorized action and verifying its postcondition. Source-level execution stays
synchronous: a program yields a typed Effect request, the host performs work,
and a validated reply resumes the same execution with its original Fuel budget.

Hosts supply their operation catalog, object adapter, live authority and event
loop. The [local host protocol draft](docs/host-protocol.md) defines opaque
object references, historical observations and revision-checked action attempts;
the reference GUI and CLI models exercise the shared boundaries. A concrete GUI
framework integration remains a separate gate. No Gewyvern, Leserpent, SQLite,
Node.js, Python or shell runtime is required by this workspace.

## Try It

Install Rust through rustup. The repository pins Rust 1.98.0 and its formatting
and lint tools in `rust-toolchain.toml`.

```sh
cargo fetch --locked
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- check examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/two_effects.lese --allow stdio
```

The CLI checks or runs `.lese` programs using the same compiler and VM as Rust
embedding. The arithmetic example prints `42`; the Effect example prints two
lines through the CLI's explicitly granted `std.print` host operation. `check`
never invokes host callbacks. Read the [language and CLI guide](docs/language.md)
for scalar values, helpers, control flow, Fuel and the supported profile.

## Embed From Rust

```sh
cargo run --example embedded_counter --locked --offline --profile lean
cargo run --example protocol_hosts --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean -- --queued
```

The [minimal embedded host](crates/leselang-vm/examples/embedded_counter.rs)
registers a callback through `leselang_vm::embedding` and explicitly drives
`poll → authorize → dispatch → accept`. It evaluates this program and produces
**42** after two host calls. `counter.add` is implemented by the example's Rust
adapter, not the VM:

```text
fn increment(n: integer) = bind(receipt: counter.add(amount: n), body: receipt)
fn main() = bind(first: increment(n: 19), body: increment(n: add(left: first, right: 4)))
```

The queued mode interleaves two independent local VMs over shared immutable IR.
It is a bounded, single-threaded host loop, not an implicit async runtime.
See the [embedding guide](docs/embedding.md) for cancellation and authority tests.
The `protocol_hosts` example discovers and binds a native object in source, then
passes it to observation/action calls. CLI and headless GUI hosts use distinct
native state and explicit event pumps; conformance tests route between two
objects and reject replies from the wrong object.
An action acknowledgement records submission; a later observation verifies the
changed state. Native object operands are an explicit HIR adapter opt-in; the
high-level embedding API remains scalar.

## Packages

| Crate | Responsibility |
| --- | --- |
| `leselang-syntax` | Lexer, parser, source spans and diagnostics |
| `leselang-runtime-core` | Host-neutral values, fuel, lifetime and protocol primitives |
| `leselang-hir-core` | Typed IR, compilation, evaluation and reply-gated sessions |
| `leselang-hir` | Public native HIR facade |
| `leselang-vm` | Scalar `leselang` CLI, optional high-level embedding API, native `Vm` and scoped `LocalVm` facade |

Start with `leselang_vm::embedding` for scalar operation registration, compilation
and explicit session handoffs. Use `leselang_vm::native` for custom HIR/catalog
adapters and direct VM control. Both modules remain available without feature
flags. Default and all-features builds are native; the old `leserpent-profile`
and `sqlite-journal` features are not available here.
The [separate consumer workspace](consumer/Cargo.toml) demonstrates direct
embedding without access to a product repository.

## Development

```sh
cargo test --workspace --all-features --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --locked --offline --profile lean
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the complete local checks. CI runs
formatting, tests, strict Clippy and example execution on Linux and macOS.
Both workspaces share one ignored build directory to avoid duplicate caches.

## Scope And Status

This is the independent native language foundation, initially extracted from
Gewyvern. It is not a complete replacement for Leserpent's product evaluator,
SQLite journal, wire protocols or GUI integration. A general durable runtime,
full declared-language parity and a stable FFI ABI remain separate milestones.
Native callbacks are trusted Rust code; fuel is not a callback sandbox and
cancellation does not roll back committed external effects.

The independent package line starts at **0.1.0**. All five crates and the separate
consumer share this version; it is independent of Gewyvern's product numbering.
Historical extraction records retain their original source versions and hashes.
Packages remain `publish = false` until a registry release is deliberately prepared.
Use local paths now; a Git dependency must pin an actual commit after publication.

- [Architecture and ownership](docs/architecture.md)
- [Local host protocol and acceptance gates](docs/host-protocol.md)
- [Language and CLI guide](docs/language.md)
- [Embedding a host](docs/embedding.md)
- [Extraction provenance and product handoff](docs/migration.md)
- [Security boundaries](SECURITY.md)

## License

[MIT](LICENSE). The original copyright notice is preserved.
