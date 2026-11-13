# Leselang

An embeddable agent control language with GUI control as its first host target
and support for CLI hosts. LeseLang separates observing an object, submitting an
authorized action and verifying its postcondition. Source-level execution stays
synchronous: a program yields a typed Effect request, the host performs work,
and a validated reply resumes the same execution with its original Fuel budget.

Hosts supply their operation catalog, object adapter, live authority and event
loop. The [local host protocol draft](docs/host-protocol.md) defines opaque
object references, historical observations and revision-checked action attempts;
the reference GUI and CLI models exercise the shared boundaries. The separate
[AppKit host](docs/appkit-host.md) applies the protocol to real macOS buttons and
editable text fields. The [Accessibility host](docs/macos-ax-host.md) provides
scoped profiles for a TextEdit document and a Calculator window in other processes.
Its opt-in [controls mode](docs/macos-ax-controls.md) discovers individual
buttons and text controls and gives them native references. An explicit
[application and window selector](docs/macos-ax-applications.md) connects that
same catalog to other running macOS application bundles.
The separate [widgets catalog](docs/macos-ax-widgets.md) adds scoped popup menus,
checkbox state, item selection and scrollbar positioning without changing the
existing controls interface.
No Gewyvern, Leserpent, SQLite,
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
Editors and agents can select `check --diagnostic-format json FILE` or
`run --diagnostic-format json FILE` for versioned JSON Lines diagnostics on
standard error. Default text diagnostics and program output remain available;
the guide specifies coordinates, failure phases and compatibility rules.

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
The same module exposes [structured diagnostics](docs/embedding.md#structured-diagnostics)
with host-neutral categories and source spans, so custom Rust hosts can present
errors without decoding CLI text or walking nested compiler errors themselves.
The `protocol_hosts` example discovers and binds a native object in source, then
passes it to observation/action calls. CLI and headless GUI hosts use distinct
native state and explicit event pumps; conformance tests route between two
objects and reject replies from the wrong object.
An action acknowledgement records submission; a later observation verifies the
changed state. Native object operands are an explicit HIR adapter opt-in; the
high-level embedding API remains scalar.

## Run A Real GUI Host

On macOS 14+ with Xcode Command Line Tools and a logged-in desktop session:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --demo --hold
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --self-test
```

The [AppKit adapter](hosts/appkit/Cargo.toml) opens a native form with two
`NSButton` controls and an editable `NSTextField`. LeseLang lists registered
controls, observes and edits the input, clicks Save, and verifies the value
captured by its real target/action. Both actions require explicit native pumps.
`--hold` keeps the window open; `--button-demo` retains the original button flow.
Use `--run hosts/appkit/examples/form.lese` to execute an independent source file
through this host's catalog with a 16 KiB UTF-8 source limit, 1,000 initial Fuel
and a scalar final result. The file runner accepts programs with their own
results and control choices; the built-in demonstrations add their own state
assertions. `hosts/appkit/test-native.sh` checks the native bridge directly.
The adapter is a separate workspace, so AppKit and its small Objective-C bridge
do not become dependencies of the five language crates or the independent
consumer. See the [AppKit guide](docs/appkit-host.md) for prerequisites,
execution boundaries and the scope of the real GUI tests.

## Run A Scoped External GUI Host

The [macOS Accessibility adapter](docs/macos-ax-host.md) has TextEdit and
Calculator profiles using the same typed host driver. The TextEdit profile
targets a plain-text document already open in the application. It checks the
exact document URL, process instance and Accessibility element before reading
or replacing text.

```sh
cargo build --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean
target/lean/leselang-macos-ax-host --check-permission
target/lean/leselang-macos-ax-host --prepare /tmp/leselang-ax-demo.txt
```

Open that newly created file in TextEdit, then run
`target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt`.
The default source finds and observes the document, explicitly pumps a text
replacement, and observes again to verify `LeseLang cross-process ✓` followed by
a newline. This four-Effect flow has been verified against a real TextEdit
window. It requires Accessibility permission for the host process; the host
checks access without prompting or changing permissions. `--prepare` refuses to
overwrite an existing file. TextEdit may autosave the disposable test document.

The Calculator profile uses a pinned Basic-mode window and real button actions:

```sh
target/lean/leselang-macos-ax-host --calculator
```

Open Calculator with the supported English Basic-mode UI and unit conversion
off first. The default source requires display `0`, then explicitly presses
`one`, `add`, `two` and `equals`, observing after each action to verify display
`3` and last expression `1 + 2`.
It does not clear an existing calculation or restore application state afterward.
Calculator starts with 2,000 Fuel; TextEdit keeps its 1,000-Fuel budget.
The ten-Effect Calculator flow has been verified against Calculator 12.0 on
macOS 27.0.1, including separate inspection of the result and last expression
in the real window. `--run FILE` supplies independent source for either selected profile.

For individual control discovery, opt into the shared controls catalog:

```sh
target/lean/leselang-macos-ax-host --calculator --controls --discover
target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt --controls --discover
```

This prints a read-only inventory of buttons, text areas, text fields and static
text in the selected scope. Use exact role, identifier and label selectors in
source, then run with `--controls --run FILE`. Each resolved control receives
its own native reference; `gui.press` and `gui.set_text` require its observed
revision. A change elsewhere in the observed scope also invalidates a queued
action. Discovery keys belong to one connection and cannot be copied into a
later CLI run. Controls mode requires explicit discovery or source, starts with
10,000 Fuel and has no default action. See the [control guide](docs/macos-ax-controls.md)
for selectors, limits, examples and failure behavior.
The controls catalog has completed a real 17-Effect `8 + 7 = 15` Calculator
flow and a four-Effect TextEdit replacement, each with separate desktop
verification. A TextEdit source also exercised `list → find → observe` through
connection-local keys.

To inspect another running application's windows, select its exact bundle:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Stickies.app --windows
```

Then combine `--app` with exact window title, identifier or local-document
selectors and `--controls --discover` or `--controls --run FILE`. An optional
PID disambiguates multiple instances of that same executable. The host pins
the selected process and native window; it does not choose by focus or switch
to a same-title replacement. See the [application selection guide](docs/macos-ax-applications.md)
for the Rust API, window keys and independent desktop verification.
The generic path has completed a six-Effect flow in Stickies 10.3 with two
text replacements and separate result inspection. Its selected window remained
bound while the note's content-derived title changed.

For menus, checkboxes, list selection and scrolling, use the optional
[`--widgets` catalog](docs/macos-ax-widgets.md) with the same explicit
application/window selection. Inspect the actual widget capabilities before
running source and request a new observation after each operation. Menu scope
remains within the selected window's supported popup relationships.
The Finder reference uses two explicitly selected windows: View Options for
menu and checkbox changes, and the dedicated folder window for exact-value
discovery, row selection and scrolling. Separate observations and desktop
inspection confirmed the final state; the guide records the menu's delayed update and the
explicit later read used before continuing.
Rust embedders can opt into [bounded observation waiting](docs/macos-ax-observation-wait.md)
for a selected pending `gui.observe` request. The host explicitly advances the
wait with its clock and original VM Fuel; it adds no operation or mutation retry.
For [source-declared waiting](docs/macos-ax-source-waits.md), the separate
`--widgets --wait-values` catalog adds `gui.wait_value`: source supplies the
expected optional value and bounded attempts/interval/timeout, while the host
still explicitly drives the clock and wait steps. The original widgets catalog
keeps its 13 operations.
The independent [state-wait catalog](docs/macos-ax-state-waits.md), selected by
`--widgets --wait-states`, includes value waiting and adds typed checkbox,
selection and scrollbar-position waits. It rejects unavailable state instead
of interpreting it as `false` or `0`; the value-only catalog remains unchanged.
For windows with controls implemented by a helper process, the
[auxiliary-process guide](docs/macos-ax-auxiliary-processes.md) describes explicit
PID/start-time/executable grants and shared revocation. These grants retain the
selected window boundary; default widget connections still require the same PID.
The Preview reference completed two observed edits to its helper-owned Export
name field in six Effects, with separate desktop verification and panel cancellation.

Unlike the application-owned form, the external adapter can detect only
observed changes. It cannot detect every intervening A→B→A change or make its
read/action sequence atomic with another application. Read the [adapter guide](docs/macos-ax-host.md)
for the exact scope, failure behavior and separate desktop acceptance procedure.

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
The workspaces share one ignored build directory to avoid duplicate caches.
CI also builds and lints the AppKit host and runs its non-GUI package tests on
macOS. Its real GUI self-test and native bridge tests require a desktop session
and are separate explicit checks.
The Accessibility host's deterministic backend and command-line tests run on
Linux and macOS; its actual cross-process workflows additionally require a desktop
and permission for the executing host. Its separate native boundary checks run
on macOS without remote GUI calls.

## Scope And Status

This is the independent native language foundation, initially extracted from
Gewyvern. It is not a complete replacement for Leserpent's product evaluator,
SQLite journal, wire protocols or arbitrary GUI automation. The AppKit host
controls its own application objects. The separate Accessibility host targets
a configured TextEdit document, Calculator window or explicitly selected
running application/window and checks external state through bounded snapshots.
The opt-in controls mode uses the same discovery and operation catalog inside
each selected scope. Its selectors and admitted operations still depend on the
application's actual Accessibility layout and the adapter's bounds. Each
additional application and platform needs its own compatibility evidence.
A general durable runtime,
full declared-language parity and a stable FFI ABI remain separate milestones.
Native callbacks are trusted Rust code; fuel is not a callback sandbox and
cancellation does not roll back committed external effects.

The independent package line starts at **0.1.0**. All five crates, the separate
consumer and the reference hosts share this version; it is independent of
Gewyvern's product numbering.
Historical extraction records retain their original source versions and hashes.
Packages remain `publish = false` until a registry release is deliberately prepared.
Use local paths now; a Git dependency must pin an actual commit after publication.

- [Architecture and ownership](docs/architecture.md)
- [Local host protocol and acceptance gates](docs/host-protocol.md)
- [Real macOS AppKit host](docs/appkit-host.md)
- [Scoped macOS Accessibility host](docs/macos-ax-host.md)
- [Scoped control discovery and operations](docs/macos-ax-controls.md)
- [Selecting macOS applications and windows](docs/macos-ax-applications.md)
- [Menus, checks, selection and scrolling](docs/macos-ax-widgets.md)
- [Explicit auxiliary process grants](docs/macos-ax-auxiliary-processes.md)
- [Language and CLI guide](docs/language.md)
- [Embedding a host](docs/embedding.md)
- [Extraction provenance and product handoff](docs/migration.md)
- [Security boundaries](SECURITY.md)

## License

[MIT](LICENSE). The original copyright notice is preserved.
