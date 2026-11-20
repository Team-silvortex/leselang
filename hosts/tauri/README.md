# Tauri Reference Host

The application owns registered TypeScript objects; Rust owns compilation,
the VM, Fuel and explicit Effect authorization. This independent workspace
contains a portable execution adapter and an optional macOS desktop shell.

## Choose An Entry

| Goal | Guide |
| --- | --- |
| Run the original one-shot form | [TypeScript host](../../docs/typescript-host.md#build-and-run) |
| Use the application conversation with offline or model planning | [Agent entry](../../docs/agent-entry.md#run-the-interactive-reference) |
| Use the same agent with a second TypeScript GUI | [Task board](../../docs/taskboard-host.md) |
| Use multiple named inputs in a real TypeScript GUI | [Article application](../../docs/article-host.md) |
| Register objects in another JS/TS application | [JavaScript SDK](../../docs/javascript-sdk.md) |
| Run the same bridge without a WebView | [Independent Node consumer](../../consumer/README.md) |
| Launch one real VM worker per task from an existing Node application | [Node-owned agent](../../docs/node-agent-host.md) |

The conversation uses [sequential workflows](../../docs/agent-workflows.md) for
up to eight declared tasks per plan. Each step has its own VM, Fuel and authority;
verification must finish before the next step starts. Earlier effects remain
when a later step fails or is cancelled. Offline commands are one-step workflows.
Model mode enables verified-result bindings and explicit result requirements:
a later task can consume an earlier result or require its exact value before
executing. A failed requirement stops the workflow before the target action;
the conversation retains the comparison and earlier execution evidence.
Natural-language planning uses an explicitly configured
[model transport](../../docs/model-provider.md); failure does not select another
planner or replay application work.

## Build And Inspect

From the repository root, with pinned Rust/npm dependencies already installed:

```sh
npm run build --prefix hosts/tauri
cargo build --manifest-path hosts/tauri/Cargo.toml --locked --offline --profile lean
target/lean/leselang-tauri-host --describe-agent
target/lean/leselang-tauri-host --describe-agent-observed
```

`--describe-agent` retains the original four-task v1 catalog.
`--describe-agent-observed` adds the opt-in v2 `read_message` observation contract;
the conversation page explicitly selects this five-task catalog. Both metadata
commands work without launching a GUI. On a logged-in macOS desktop, launch the
conversation explicitly with `target/lean/leselang-tauri-host --agent`.

The separate `--agent-board` entry opens an in-memory task board with its own
five-task v2 catalog, available offline through `--describe-agent-board`.
The two applications share the conversation, provider and verifier while native
startup pins each application's page and report identity; see the
[task board guide](../../docs/taskboard-host.md).

The `--agent-article` entry opens an in-memory title/content editor and publication
preview. Its separate v3 catalog is available through `--describe-agent-article`.
It uses the same conversation and providers with the explicit named-task native
entry; see the [article guide](../../docs/article-host.md).

## Source Map

| Location | Responsibility |
| --- | --- |
| `src/lib.rs`, `catalog.rs`, `gateway.rs`, `host.rs`, `task.rs` | Application wrappers and compatible re-exports of the standalone registered-object core |
| `src/agent_semantics.rs`, `board_semantics.rs`, `article_semantics.rs`, `application.rs` | Application declarations and host-selected identity |
| `src/desktop/mod.rs` | Conditional desktop module boundary |
| `src/desktop/agent.rs`, `model.rs` | Conversation IPC/lifetimes and native model transport |
| `src/desktop/form.rs`, `wire.rs` | Original form and shared transport correlation |
| `src/desktop/*/tests.rs` | Tests of the corresponding private modules |
| `frontend/src/app.ts`, `agent-app.ts`, `agent-board-app.ts`, `agent-article-app.ts` | One-shot and three application chat entry points |
| `frontend/src/form/`, `board/`, `article/` | Separate application models and views |
| `frontend/src/agent/` | Host/provider adapters and task/workflow result displays |
| `frontend/src/bridge.ts`, `tauri-api.ts` | SDK exports and typed Tauri IPC boundary |
| `frontend/public/`, `frontend/tests/` | Static pages and application tests |
| `frontend/{build,clean}.mjs` | Build assets and clear only generated frontend output |
| `frontend/dist/` | Ignored generated assets; edit source instead |
| `tests/model-service.mjs` | Labelled loopback protocol fixture; no online model inference |
| `tests/`, `examples/` | Public Rust/CLI tests, explicit desktop tests and standalone carriers |
| `capabilities/`, `permissions/`, `tauri.conf.json` | Desktop IPC exposure and local page policy |

The frontend imports the public [`@leselang/bridge`](../../packages/javascript/README.md)
entry. The build copies its compiled browser module tree into `frontend/dist/sdk/`
so private relative imports resolve. Shared catalog and source goldens live in
[`fixtures/host-semantics/`](../../fixtures/host-semantics/README.md).

The portable gateway, native authority, task compiler and VM driver live in
[`../registered`](../registered/README.md). Tauri's existing Rust APIs re-export
those exact types and preserve their behavior. Form, task-board and article identities,
semantic catalogs, desktop IPC and report naming remain here. The old
`examples/agent_worker.rs` delegates to the standalone `leselang-agent-worker`;
Node applications no longer need this workspace to build their execution worker.

## Check Changes

```sh
sh scripts/check.sh javascript
sh scripts/check.sh tauri-portable
sh scripts/check.sh tauri-desktop
```

Run the last group on macOS after building frontend assets. The
[check guide](../../scripts/README.md) defines each group's prerequisites.
`npm test --prefix hosts/tauri` checks the frontend only; `npm run test:all --prefix hosts/tauri` retains the combined SDK/frontend npm check. Neither these checks
nor the desktop group launches the interactive GUI. Separate desktop procedures
are in the host guides, with dated [JavaScript/Tauri](../../docs/acceptance/javascript.md)
and [agent](../../docs/acceptance/agents.md) evidence in the archive.
