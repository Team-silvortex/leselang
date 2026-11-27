# LeseLang

**Project version 0.2.3** · [Source revision `a27099e`](https://github.com/Team-silvortex/leselang/commit/a27099e4792124c4868e2870a7787e869d92fb0f)

An embeddable agent control language with GUI control as its first host target
and support for CLI hosts. An application declares its operations and task
semantics; an agent can discover that contract, generate a typed LeseLang
program, execute authorized Effects and verify the observed result.

The language owns compilation, the VM and Fuel. Hosts own application objects,
live permissions and event processing. Execution keeps the explicit
`poll → authorize → dispatch → accept` boundary. There is no implicit scheduler,
automatic action retry or rollback of committed effects.

Nuis is the planned primary application host. Rust, JS/TS and C# hosts
exercise the [shared scalar operation contract](docs/operations-host-protocol.md).
Independent Python, C and C++ examples run the same meter program against a
separate evaluator. [Run the three Consumers](docs/operations-host-protocol.md#python-c-and-c-consumers)

## Choose A Starting Point

| I want to… | Start here |
| --- | --- |
| Find the current capability status or next development task | [Project status](STATUS.md) and the [development mainline](docs/development-mainline.md) |
| Write and run a LeseLang program | [Language and CLI](docs/language.md), or the quickstart below |
| Implement a host in a new language | [Scalar application host contract and interoperability check](docs/operations-host-protocol.md) |
| Embed the compiler and VM in Rust | [Rust embedding](docs/embedding.md) |
| Expose JavaScript or TypeScript application controls | [JavaScript SDK](docs/javascript-sdk.md) |
| Give an agent typed JS/TS application commands | [Application operations](docs/application-operations.md) |
| Give an agent C# application commands | [.NET host and three-host conformance example](docs/dotnet-host.md) |
| Give an application an agent conversation entry | [Agent entry](docs/agent-entry.md), then [sequential workflows](docs/agent-workflows.md) |
| Build or use a GUI adapter | [Reference hosts](hosts/README.md) and [local host protocol](docs/host-protocol.md) |
| Contribute to the repository | [Architecture](docs/architecture.md) and [contributor checks](CONTRIBUTING.md) |

The [documentation index](docs/README.md) separates guides, protocol references,
contributor requirements and dated acceptance evidence.

## Repository Map

| Location | Responsibility |
| --- | --- |
| [`crates/`](crates) | The five language crates: syntax, runtime contracts, HIR and VM |
| [`packages/javascript/`](packages/javascript/README.md) | Framework-independent JavaScript bridge, agent sessions and semantic planning |
| [`packages/dotnet/`](packages/dotnet/) | .NET operation declarations, live grants and process transport to the same Rust VM |
| [`hosts/`](hosts/README.md) | Standalone registered-object worker plus AppKit, macOS Accessibility and Tauri adapters |
| [`consumer/`](consumer/README.md) | Independent Rust and JavaScript applications exercising public APIs |
| [`fixtures/host-semantics/`](fixtures/host-semantics/README.md) | Shared catalog and generated-program compatibility fixtures |
| [`examples/`](examples) | Small LeseLang programs for the CLI |
| [`scripts/`](scripts/README.md) | Shared local and CI check groups |
| [`project/status/`](project/status/catalog.json) and [`tools/leselang-dev/`](tools/leselang-dev/) | Capability inventory, evidence and independent Rust query tool |
| [`docs/`](docs/README.md) | Guides, contracts, contributor reference and acceptance records |

Cargo workspaces remain separate and share only the ignored `target/` build cache.
The language workspace does not require a GUI toolkit, Node.js or a product
checkout. Individual reference hosts have their own documented prerequisites.

## Try It

Install Rust through rustup. `rust-toolchain.toml` pins Rust 1.98.0 and its
formatting and lint tools. From the repository root:

```sh
cargo fetch --locked
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- check examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/two_effects.lese --allow stdio
```

The arithmetic example returns `42`. The Effect example prints two lines through
its explicitly granted `std.print` host operation. `check` compiles without
invoking host callbacks. The [language guide](docs/language.md) covers named
entries, typed inputs, control flow, Fuel, formatting and structured diagnostics.

Use `leselang describe` to inspect the CLI's versioned operation catalog. Hosts
can additionally declare [task semantics and verification](docs/host-semantics.md).
The independent [Rust catalog agent](docs/agent-workflow.md) demonstrates
catalog discovery, program generation, one explicit execution and verification
without a model call.

## Embed From Rust

```sh
cargo run --example embedded_counter --locked --offline --profile lean
```

The [minimal embedded host](crates/leselang-vm/examples/embedded_counter.rs)
registers a callback through `leselang_vm::embedding`, explicitly drives two host
Effects and returns `42`. The [embedding guide](docs/embedding.md) covers host
registration, compilation, named task inputs, sessions, cancellation and failure
semantics. Existing low-level `OperationCatalog`, `Vm` and `LocalVm` interfaces
remain available.

For custom native objects and catalog adapters, use `leselang_vm::native` and the
[local host protocol](docs/host-protocol.md). Native object identity, authority
and revision checks remain host responsibilities.

## Run A Real GUI Host

The separate [AppKit host](docs/appkit-host.md) controls application-owned macOS
buttons and text fields. Its guide covers the native build, explicit event pump,
source-file execution and desktop self-test. It requires macOS 14+, Xcode
Command Line Tools and a logged-in desktop session.

## Control TypeScript In A Real WebView

The [JavaScript SDK](docs/javascript-sdk.md) registers application-owned objects
behind the same operation boundary. The application owns its input and Promise
handlers; Rust retains compilation, VM execution and Fuel. The independent
[Node consumer](consumer/README.md) exercises this integration without a GUI.

The [Tauri reference host](hosts/tauri/README.md) adds a real macOS WebView and an
[in-app agent entry](docs/agent-entry.md). Its offline command provider needs no
model credits. An optional [model provider](docs/model-provider.md) selects
host-declared tasks from natural language. [Sequential workflows](docs/agent-workflows.md)
admit the complete plan, verify each step and can bind an earlier verified
result into a later task's input. Optional result requirements stop a later step
unless an earlier verified result equals the requested value.

The original `--describe-agent` exposes four tasks with fixed v1 results.
`--describe-agent-observed` exposes the opt-in five-task v2 catalog used by the
conversation page. Its read task verifies current text against actual observation
evidence, so the same declaration can return different current values.

The separate [task board](docs/taskboard-host.md) uses that same conversation,
provider and execution path for a different TypeScript GUI. It declares task
status reads and one-time completion, with conditional workflows that stop when
a task is already done. Launch it with `--agent-board`; inspect its independent
catalog with `--describe-agent-board`.

## Run A Scoped External GUI Host

The [macOS Accessibility host](docs/macos-ax-host.md) controls an explicitly
selected external application/window. Start with its TextEdit or Calculator
profile, then use [application selection](docs/macos-ax-applications.md),
[control discovery](docs/macos-ax-controls.md) or [widget operations](docs/macos-ax-widgets.md)
for supported layouts. [Auxiliary-process grants](docs/macos-ax-auxiliary-processes.md)
and [bounded waits](docs/macos-ax-observation-wait.md) are explicit opt-ins.

External state checks are snapshot based. They cannot make a read/action pair
atomic with another application or detect every intervening A→B→A change.
Application compatibility requires its own evidence; the
[acceptance archive](docs/acceptance/README.md) records tested layouts and results.

## Packages

| Crate | Responsibility |
| --- | --- |
| `leselang-syntax` | Lexer, parser, source spans and diagnostics |
| `leselang-runtime-core` | Host-neutral values, Fuel, lifetime and protocol primitives |
| `leselang-hir-core` | Typed IR, compilation, evaluation and reply-gated sessions |
| `leselang-hir` | Public native HIR facade |
| `leselang-vm` | Scalar CLI, high-level embedding API, native `Vm` and scoped `LocalVm` facade |

The `embedding` and `native` modules are available without feature flags. Default
and all-features builds use the native implementation; product-only
`leserpent-profile` and `sqlite-journal` features are not available here.

## Development

With pinned dependencies already installed:

```sh
sh scripts/check.sh --list all
sh scripts/check.sh language
sh scripts/check.sh consumer
```

[CONTRIBUTING.md](CONTRIBUTING.md) maps changed layers to checks.
[Testing requirements](docs/testing.md) retain the complete regression and
explicit desktop procedures. Local checks and CI share the
[check groups](scripts/README.md); running them does not install dependencies or
launch GUI applications. Dated desktop evidence is maintained separately from
current setup instructions.

## Scope And Status

The [development tensor](STATUS.md) is the authority for current capability
status, evidence and mainline selection. Architecture guides describe design
boundaries and dated acceptance records preserve observations from individual runs.

LeseLang is an independent native language foundation, initially extracted from
Gewyvern. Its language crates have no Gewyvern, Leserpent, SQLite or GUI product
dependency. A durable runtime, full declared-language parity, broader application
and platform support, and a stable FFI ABI remain separate milestones.
Kyuubiki Workbench is the planned [first formal product trial](docs/architecture.md#first-formal-product-trial)
of the protocol, with the adapter and workflow acceptance owned by Kyuubiki.
Native callbacks are trusted code: Fuel does not sandbox them, and cancellation
does not roll back committed external effects. See [security boundaries](SECURITY.md).

The Rust crate, npm package and Tauri application manifests in this revision
declare package version `0.1.0`. The Rust crates use `publish = false`.
Rust consumers can use a local checkout or pin the project revision shown
above. [Dependency examples](docs/embedding.md#dependency-setup) ·
[Version history and provenance](docs/migration.md#origin)

## License

[MIT](LICENSE). The original copyright notice is preserved.
