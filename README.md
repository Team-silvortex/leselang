# Leselang

An embeddable Rust control language for explicitly adapted hosts. Leselang keeps
source-level execution synchronous: a program yields a typed effect request,
the host performs authorized work, and a validated reply resumes the same
execution with its original state and fuel budget.

GUI automation is one application, not a built-in GUI toolkit or automatic
binding. Hosts supply their own operation catalog, authority, event loop and
native implementation. No Gewyvern, Leserpent, SQLite, Node.js, Python or shell
runtime is required by this workspace.

## Try It

Install Rust through rustup. The repository pins Rust 1.98.0 and its formatting
and lint tools in `rust-toolchain.toml`.

```sh
cargo fetch --locked
cargo run --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean -- --queued
```

The first example evaluates this program and produces **42** after two host
calls. `counter.add` is implemented by the example's Rust adapter, not the VM:

```text
fn increment(n: integer) = bind(receipt: counter.add(amount: n), body: receipt)
fn main() = bind(first: increment(n: 19), body: increment(n: add(left: first, right: 4)))
```

The queued mode interleaves two independent local VMs over shared immutable IR.
It is a bounded, single-threaded host loop, not an implicit async runtime.
See the [embedding guide](docs/embedding.md) for cancellation and authority tests.

## Packages

| Crate | Responsibility |
| --- | --- |
| `leselang-syntax` | Lexer, parser, source spans and diagnostics |
| `leselang-runtime-core` | Host-neutral values, fuel, lifetime and protocol primitives |
| `leselang-hir-core` | Typed IR, compilation, evaluation and reply-gated sessions |
| `leselang-hir` | Public native HIR facade |
| `leselang-vm` | Public native `Vm` and scoped `LocalVm` facade |

Start with `leselang_vm::native`. Default and all-features builds are native;
the old `leserpent-profile` and `sqlite-journal` features are not available here.
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
- [Embedding a host](docs/embedding.md)
- [Extraction provenance and product handoff](docs/migration.md)
- [Security boundaries](SECURITY.md)

## License

[MIT](LICENSE). The original copyright notice is preserved.
