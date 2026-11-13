# Contributing

Keep the language foundation independent of host products. New operations belong
in a host adapter unless they are genuine language or generic protocol primitives.
Do not add a GUI toolkit, account service, product DTO or persistence policy to
the language crates. Existing native APIs and tests are the executable contract;
historical extraction notes do not override current code.

## Verification

Fetch the pinned dependencies once, then validate the language and consumer
workspaces offline:

```sh
cargo fetch --locked
cargo fetch --manifest-path consumer/Cargo.toml --locked
cargo fetch --manifest-path hosts/appkit/Cargo.toml --locked
cargo fetch --manifest-path hosts/macos-ax/Cargo.toml --locked
cargo fmt --all -- --check
cargo fmt --manifest-path consumer/Cargo.toml --all -- --check
cargo fmt --manifest-path hosts/appkit/Cargo.toml --all -- --check
cargo fmt --manifest-path hosts/macos-ax/Cargo.toml --all -- --check
cargo test --workspace --all-features --locked --offline --profile lean
cargo clippy --workspace --all-features --all-targets --locked --offline --profile lean -- -D warnings
cargo test --manifest-path consumer/Cargo.toml --locked --offline --profile lean
cargo clippy --manifest-path consumer/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- check examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/arithmetic.lese
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/two_effects.lese --allow stdio
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/entry_points.lese --entry preview --allow stdio
cargo run -p leselang-vm --bin leselang --locked --offline --profile lean -- run examples/entry_points.lese --entry report --allow stdio
cargo run --example embedded_counter --locked --offline --profile lean
cargo run --manifest-path consumer/Cargo.toml --example embedded_counter --locked --offline --profile lean
cargo run --example protocol_hosts --locked --offline --profile lean
cargo run --manifest-path consumer/Cargo.toml --example protocol_hosts --locked --offline --profile lean
cargo test --example native_counter --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean
cargo run --example native_counter --locked --offline --profile lean -- --queued
```

The consumer is deliberately a separate Cargo workspace. Its repository-boundary
test checks actual Cargo metadata, including all-features builds, so a sibling
checkout or optional product dependency cannot silently become a prerequisite.

Use the `lean` profile for local checks. All four workspaces share the repository's
`target/` through `.cargo/config.toml`; their independence concerns dependencies
and acceptance, not separate build caches. See [build output and cleanup](#build-output-and-cleanup)
below before changing output paths. No containers or external server are needed
for these native tests.

CLI integration tests run with the workspace suite. The smoke commands above
check a pure program, evaluate its result and execute two explicitly granted
stdout Effects. Keep the examples in `examples/` executable when changing the
scalar language or CLI host; document profile changes in
[the language guide](docs/language.md).

Keep `embedding::diagnostics` host-neutral. Structured categories and source
locations must come from the public error view, while terminal escaping, CLI
capability hints and JSON Lines presentation belong to the executable. Cover
both default text and opt-in JSON for syntax, compilation, input, usage and
runtime failures; preserve stdout and previously delivered Effects. Test
Unicode scalar columns, UTF-8 byte spans and absent runtime locations without
turning diagnostic messages into a parsing contract. Verify the API from the
independent Consumer as well as the CLI. Dotted host operation names are exact
catalog keys, so parser admission must remain consistent with registration,
including names with more than two segments.

Explicit entry selection must use the existing catalog compiler without
synthetic source or an extra runtime wrapper. Keep default `main` behavior,
complete cold/unused declaration checks and the ban on calling the selected
entry covered. Test named-entry execution against equivalent `main` source for
results, original Fuel and partial Effect delivery, including revocation and
cancellation. Verify owned entry metadata and inferred scalar result types from
the independent Consumer, and `--entry` parsing and text/JSON failures from the
CLI. Program metadata is descriptive and must not authorize execution.

Typed task ingress is an explicit `compile_task` / `start_with_arguments` path.
Keep legacy parameterless admission unchanged, require every named input and
check types, scalar bounds and binding capacity before session creation. Test
reordered inputs, all six scalar kinds, foreign programs and malformed inputs
with no host work. The shared binder must move payloads rather than clone them;
compare task-local reads against native `LocalVm` initial bindings at every
Fuel cutoff. Preserve original Fuel through two Effects, captures, revocation,
cancellation, late replies and partial side effects. The independent
[`task_inputs` tests](consumer/tests/task_inputs.rs) exercise these contracts.
For CLI `--text`, cover literal Unicode/quote/backslash/newline values, all
option and size limits, source stdin, quiet `check`, JSON input diagnostics and
zero output on rejected inputs.

Native helper type annotations require the typed catalog compiler and explicit
host type-name resolution. Preserve the old compiler's scalar-only profile and
default refusal of native type names. Test exact native type matching, cold
errors, helper hygiene and pure previously bound operands; no helper parameter
may manufacture an object identity or admit an Effectful argument. GUI adapters
must retain runtime object/lease/revision/authority checks when a helper receives
an `object` or `snapshot`. Keep source parameter inputs separate from native
object values and keep native field projections scalar.

Source formatting uses `leselang_syntax::format_source`, while the older
AST-based `format` retains its comment-erasing compatibility contract. Keep
non-whitespace token spellings and order, comment text and neighboring-token
relationships, string contents and formatting idempotence covered by syntax
tests. Verify formatted programs through an independent Consumer, including
their results, Effect order, original Fuel and cold-code admission failures.
The CLI's `fmt` and `fmt --check` must not write input files or register or run
host operations. Cover input/output bounds before emitting stdout, formatting
differences, partial stdout write failures and the existing text/JSON
diagnostic contracts. Document formatting profile changes in the
[language guide](docs/language.md#format-source-without-losing-comments).

The `protocol_hosts` example runs both reference adapters against the local host
protocol. Conformance changes must preserve stale-object/revision rejection,
VM-scope revocation and the distinction between submitted actions and observed
postconditions. These headless fixtures do not establish compatibility with a
concrete GUI framework; update [the protocol draft](docs/host-protocol.md) when
its evidence or remaining gates change.

The real AppKit adapter is an independent workspace in `hosts/appkit`. Keep its
framework link flags, Objective-C bridge and widget policy out of the five
language crates. On macOS, also run:

```sh
cargo build --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean
cargo test --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean
cargo clippy --manifest-path hosts/appkit/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --self-test
hosts/appkit/test-native.sh
```

The self-test and native bridge test run on the process's main thread and use
actual AppKit controls. They require a logged-in macOS 14+ desktop session and
Xcode Command Line Tools. The ordinary package tests cover command-line ingress
and compile-time thread affinity without starting GUI work on test workers.
There is no successful headless fallback or skipped GUI test reported as proof.
CI builds and lints the host and runs its non-GUI package tests on macOS. Run
both desktop commands explicitly when changing its bridge, event pump,
discovery, identity/dependency tracking or operation policy. Preserve both the
four-Effect button compatibility example
and the source-file form workflow. See
[the AppKit guide](docs/appkit-host.md) for the adapter's tested scope.

The independent `hosts/macos-ax` workspace supplies scoped TextEdit and Calculator
Accessibility profiles over one shared typed host driver. Keep its process
selection, permissions, traversal and native bridge out of the language crates.
Preserve the existing TextEdit `Backend`, `Snapshot` and default driver entry
points when extending profiles. Run:

```sh
cargo test --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean
cargo clippy --manifest-path hosts/macos-ax/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
```

On macOS, also run `hosts/macos-ax/test-native.sh`, build the host and follow the
permission check and the relevant TextEdit or Calculator workflow in
[the Accessibility guide](docs/macos-ax-host.md). The native boundary checks
cover URL/text admission and AX error handling without remote GUI calls. Ordinary
backend tests do not establish cross-process desktop execution. Use a dedicated
new file for TextEdit. Calculator requires the supported Basic-mode UI and an
initial display of `0`; do not clear an unrelated calculation to force the demo
through. Preserve unrelated documents and application state. Do not
automatically grant Accessibility access or retry an uncertain native mutation.
Document snapshot-based freshness separately from the application-owned
AppKit host's mutation tracking: a remote read followed by a write is not an
atomic operation, and unseen A→B→A changes cannot be ruled out. Calculator
snapshots must include the display, optional last-expression dependency and the
allowed buttons' identity and enabled state. Test expression changes with an
unchanged displayed result; testing only the chosen button or the display would
miss stale calculations.

The additive `--controls` mode uses the same typed driver with per-control
native references. Keep its discovery keys connection-local and non-reusable,
preserve owner tombstones when controls disappear, and reject empty/ambiguous
selectors and cross-target replies. Test inventory reordering and replacement
separately. Scope freshness must include other controls and non-target
dependencies, including Calculator expression changes with an unchanged
display or selected button. A queued action must recheck the complete expected
scope before delivery. Exercise both controls actions, stale keys, identity
exhaustion, permission revocation, cancellation, late replies and partial
effects without weakening the original profiles' regression tests. Follow the
[control guide](docs/macos-ax-controls.md) for explicit discovery/source runs;
`--controls` has no implicit demonstration action. Record real controls-mode
desktop evidence separately from earlier profile runs and fake-backend tests.

For [generic application/window selection](docs/macos-ax-applications.md), keep
the bundle path and optional PID admission exact, preserve same-user process
executable/start checks, and require unique exact window selectors. Cover
ambiguous or absent matches, wrong-process PIDs, stale/cross-connection window
keys, title changes, reordered windows and same-title replacements. Selecting
a window pins its native identity; later reads must not rerun a selector and
silently switch targets. `--windows` remains metadata-only, and generic
controls runs require explicit window selection and source. Use dedicated test
content in another real application and inspect its final state separately
before recording desktop acceptance; native test doubles alone are insufficient.

The optional [widgets catalog](docs/macos-ax-widgets.md) preserves the original
controls API while adding typed menu, checkbox, selection and scroll operations.
Test item membership with native identities, including menu items that vanish
after a successful choice. Replies must remain correlated with the stable
owner and execution without weakening cross-target or late-reply checks. Cover
mixed checkbox values, unsupported capabilities, selection changes, non-finite
scroll ranges, invalid normalized positions and stale queued operations.
Value selectors must require an exact nonempty role, distinguish an absent
value from an empty string, and reject duplicate matches including disabled
items. Keep the Finder example sources covered directly by reference tests,
including an acknowledged menu choice whose immediate observation has not yet
updated, and stopping before a second mutation after a failed postcondition.
Desktop acceptance should use disposable document/dialog content and separately
verify each supported operation family. Opening a menu does not authorize the
global menu bar, and selecting an item must not silently activate or open it.

For [auxiliary-process grants](docs/macos-ax-auxiliary-processes.md), keep the
default same-process refusal and cover exact process identity, process restart
or PID reuse, mismatched executable/user, duplicate and excessive grants,
foreign-process boundaries, and unreachable objects in an otherwise approved
process. Revocation must fence dispatch, queued delivery and reply acceptance
without undoing delivered effects. Discovery must not grant approval or query
an unapproved helper's other windows. Native callback storage must outlive the
connection and remain confined to the owning thread.

## Build Output And Cleanup

Run repository commands with the pinned toolchain and `--profile lean`. Each
workspace defines this development profile with debug information and incremental
compilation disabled. Cargo's target directory is the parent of its profile
directories: the repository target root is `target/`, and the `lean` profile's
output is `target/lean/`. If an explicit path is needed, use
`--target-dir target --profile lean` from the repository root. Setting
`--target-dir target/lean` creates another target root: a default development
build then lands in `target/lean/debug/`, and a lean build in
`target/lean/lean/`. See Cargo's [build cache layout](https://doc.rust-lang.org/stable/cargo/reference/build-cache.html).

`CARGO_TARGET_DIR`, `CARGO_BUILD_TARGET_DIR` or a command-line override can change
the effective root. Before diagnosing duplicate outputs, inspect
`target_directory` from `cargo metadata --no-deps --format-version 1 --locked --offline`
in the relevant workspace; all four should resolve to this repository's
`target/`. Keep one cache root for normal checks instead of assigning directories
per command. The AppKit bundle script explicitly passes the repository target
root to Cargo, so its build and subsequent binary copy use the same location
even when the environment overrides the usual configuration. Cargo documents
these overrides in its [configuration reference](https://doc.rust-lang.org/stable/cargo/reference/config.html).

When no builds are running, inspect unwanted development outputs before removing
them. From the repository root, after confirming the effective target directory:

```sh
cargo clean --profile dev --dry-run
cargo clean --profile dev
```

This removes the development/test `debug/` output while retaining `lean/`.
Review the dry-run output before the destructive command. Several fingerprints
for the same crate can be valid results of tests, checks, features and separate
workspace dependency graphs; do not prune individual files in `deps/`,
`.fingerprint/` or `build/` by name or timestamp. If a historical command created
a different target root, first establish its exact path and contents before
cleaning it. Do not remove the whole repository `target/` as routine maintenance:
it also holds demo bundles and desktop fixture documents, which may still be
open in another application. Repository cleanup does not require deleting the
user's global Cargo registry or other projects' caches.

## Changes And Releases

Keep changes in the owning crate, add a focused regression, and validate the
separate consumer whenever public types, features or ownership change. Test
partial effects, stale replies and cancellation as well as the success path.
Never present a source budget as preemption of arbitrary native callbacks.

The independent version line starts at 0.1.0. Update all five crate manifests,
the separate consumer and reference hosts together; the repository-boundary test
rejects version drift.
Package numbering is separate from operation, protocol and checkpoint schema versions.
Workspace lockfiles are tracked. Update them intentionally when packages or dependencies change.
Keep MIT metadata and the LICENSE notice. Crates are not yet registry-publishable:
release preparation must choose version policy, versioned inter-crate dependencies,
package contents and a real repository URL before removing `publish = false`.

The product repository retains a compatibility baseline during handoff. Do not
develop separate native implementations in both repositories; record deliberate
backports until the product consumes an immutable Leselang revision. See
[the migration checklist](docs/migration.md).
