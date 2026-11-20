# Tauri Reference Host

The application owns the registered TypeScript objects; Rust owns compilation,
the VM, Fuel and explicit Effect authorization. This independent workspace
contains both a portable execution adapter and an optional macOS desktop shell.

| Location | Responsibility |
| --- | --- |
| `src/lib.rs`, `catalog.rs`, `gateway.rs`, `host.rs`, `task.rs` | Portable declarations, bounded transport, native identities and explicit VM driving |
| `src/agent_semantics.rs` | The reference application's declared agent tasks |
| `src/desktop/mod.rs` | Single desktop module boundary used by the executable |
| `src/desktop/agent.rs` | Conversation IPC and workflow/page lifetimes |
| `src/desktop/model.rs` | Native model configuration, bounded HTTP requests and planning cancellation |
| `src/desktop/form.rs` | Original one-shot form WebView |
| `src/desktop/wire.rs` | Shared desktop transport and correlation checks |
| `src/desktop/*/tests.rs` | Tests of the corresponding private desktop modules |
| `frontend/src/` | TypeScript application model, views and agent connection |
| `frontend/public/` | HTML pages and styles |
| `frontend/tests/` | Application tests and local fixtures |
| `tests/model-service.mjs` | Explicit loopback protocol fixture for desktop checks without model credits |
| `frontend/build.mjs` | Copy pages and the public SDK's compiled module tree |
| `frontend/dist/` | Ignored generated WebView assets; edit source instead |
| `tests/` | Public Rust, CLI and explicit desktop acceptance tests |
| `examples/` | One-shot form program and standalone Node carrier |
| `capabilities/`, `permissions/`, `tauri.conf.json` | Desktop IPC exposure and local page policy |

The frontend imports the public
[`@leselang/bridge`](../../packages/javascript/README.md) package. The build
copies its compiled modules into `frontend/dist/sdk/` for the local WebView,
preserving relative imports. Shared catalog and source goldens live in
[`fixtures/host-semantics/`](../../fixtures/host-semantics/README.md).

From the repository root, with pinned dependencies already installed:

```sh
npm test --prefix hosts/tauri
cargo test --manifest-path hosts/tauri/Cargo.toml --no-default-features --locked --offline --profile lean
```

For a macOS desktop session, build the frontend first, then:

```sh
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe-agent
target/lean/leselang-tauri-host --agent
```

See the [TypeScript host guide](../../docs/typescript-host.md) for setup and
one-shot behavior, the [agent-entry guide](../../docs/agent-entry.md) for the
conversation contract, and the [JavaScript SDK guide](../../docs/javascript-sdk.md)
for application adapter responsibilities.

For natural-language planning, configure the native model service as described
in the [model provider guide](../../docs/model-provider.md). The same conversation
entry retains the explicitly labelled offline command mode. Model failure does
not retry application work or switch planners automatically.

The conversation uses the SDK's [sequential workflow session](../../docs/agent-workflows.md)
to execute up to eight declared tasks from one model plan. It validates the
complete plan before execution and verifies each step before starting the next.
Offline commands remain one-step workflows. Each step uses an ordinary Rust VM
session with its own Fuel and authorization; failure or cancellation preserves
earlier results and stops the remaining steps.
