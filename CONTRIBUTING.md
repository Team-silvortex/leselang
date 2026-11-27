# Contributing

Keep the language foundation independent of host products. New operations belong
in a host adapter unless they are genuine language or generic protocol primitives.
Do not add a GUI toolkit, account service, product DTO or persistence policy to
the language crates. Existing native APIs and tests are the executable contract;
historical extraction notes do not override current code.

## Source Organization

Use the [repository map](README.md#repository-map) and
[architecture](docs/architecture.md#repository-organization) to choose the
owning layer. Shared catalog/source goldens belong in
[`fixtures/host-semantics/`](fixtures/host-semantics/README.md); application
declarations remain in `hosts/`, and single-application test data remains beside
its tests. Preserve golden bytes during a structural move. Update all Rust
`include_str!` paths, JavaScript fixture URLs and documentation links together.

Keep public package and module entry points stable when moving private code.
For the JavaScript SDK, verify the actual packed artifact and an external
Consumer, including declarations and private relative imports. The Tauri build
must copy the compiled SDK module tree recursively. Edit TypeScript under
`frontend/src/` and pages under `frontend/public/`; tests live in
`frontend/tests/`, and `frontend/dist/` is generated. Desktop-only Rust code
belongs in `hosts/tauri/src/desktop/`; portable host APIs must continue to build
with `--no-default-features`.

## Verification

Record a capability change in its [development tensor](docs/development-tensor.md)
cell alongside its scoped contract, evidence and next acceptance gate. Register new
coordinates in the coverage inventory. Review changes to ordered mainline goals
in `project/status/milestones.json`. Keep dated receipts in the acceptance archive.

Select the smallest complete set of checks for the layers changed. From the
repository root, inspect the shared local/CI entry point before running it:

```sh
sh scripts/check.sh --list all
sh scripts/check.sh --list language
sh scripts/check.sh language
```

| Changed layer | Check groups |
| --- | --- |
| Development catalog, evidence identity, mainline goals or planning CLI | `tensor` and the cell's validation command when its capability changes |
| Language, runtime contracts or Rust public APIs | `language`, `consumer`; also `smoke` after its AppKit/AX build prerequisites, and affected hosts when contracts change |
| Independent Rust catalog agent | `consumer`; include its affected host groups |
| Shared operation signatures, scalar encoding or worker operation profile | `consumer`, `registered`, `javascript`, `node-consumer`, `dotnet-consumer` |
| C# operation library or Consumer | `dotnet-consumer` on Windows (Git Bash/MSVC), macOS or Linux using the .NET 10 SDK |
| Registered-object Rust core or standalone worker | `registered`, `consumer`, `node-consumer`, `tauri-portable`; include `tauri-desktop` on macOS for shared execution changes |
| JavaScript SDK or Tauri frontend | `javascript`, `node-consumer`; include `tauri-portable` when the transport or task contract changes |
| AppKit adapter | `appkit`, plus `language` and `consumer` for shared behavior |
| macOS Accessibility adapter | `macos-ax`, plus `language` and `consumer` for shared behavior |
| Tauri Rust host | `tauri-portable`; also `tauri-desktop` on macOS for desktop changes |
| Full portable/build regression | `all`; platform-specific checks follow the script's documented conditions |

The [script guide](scripts/README.md) lists prerequisites and exact group scope.
Checks use the pinned toolchain, offline dependencies and the shared `lean`
profile. They do not install packages or launch GUI applications. Fetch/install
pinned dependencies deliberately before the first run. Real desktop procedures
remain explicit in the relevant host guide, including permission and application
state requirements.

[Testing and compatibility requirements](docs/testing.md) retain the complete
language, Consumer, agent and adapter regression requirements, check-group
entry points, and separate desktop checks. Follow the requirements for each changed
boundary; a successful build alone is not verification of application behavior.
Record dated acceptance in [the archive](docs/acceptance/README.md), preserving
platform, source, failure and partial-effect scope. Do not infer online model
acceptance from local protocol fixtures.

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
in the relevant workspace; all five should resolve to this repository's
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

The [README](README.md#leselang) identifies the project version and its Git
revision. When documenting a new project version, update the documentation
index, status overview and pinned dependency example to match that revision.
When changing Rust package versions, update all five crate manifests, the
separate Consumer and reference hosts together. The repository-boundary test
checks their package identity. Operation, protocol and checkpoint schemas
carry their own compatibility versions.
Workspace lockfiles are tracked. Update them intentionally when packages or dependencies change.
Keep MIT metadata and the LICENSE notice. Crates are not yet registry-publishable.
Before enabling publication, choose the package versioning policy, specify
versioned inter-crate dependencies, review the package contents and record
the repository URL in the crate metadata.

The product repository retains a compatibility baseline during handoff. Do not
develop separate native implementations in both repositories; record deliberate
backports until the product consumes an immutable Leselang revision. See
[the migration checklist](docs/migration.md).
