# Reference Hosts

Each adapter owns its operation meaning, application objects, permissions and
event processing. The language crates do not depend on these workspaces.

| Directory | Scope | Guide |
| --- | --- | --- |
| [`appkit/`](appkit) | In-process macOS controls and a main-thread native bridge | [AppKit](../docs/appkit-host.md) |
| [`macos-ax/`](macos-ax) | Explicitly scoped cross-process Accessibility control | [macOS AX](../docs/macos-ax-host.md) |
| [`tauri/`](tauri/README.md) | Registered JS objects, a Rust VM owner and desktop WebView transport | [TypeScript host](../docs/typescript-host.md) |
| [`shared/`](shared/README.md) | Source-only reference application semantic declarations | [Host semantics](../docs/host-semantics.md) |

The three adapters are independent Cargo workspaces with their own manifests
and lockfiles. `shared/` is not another crate or workspace: it holds host-owned
declarations reused by the reference adapters without adding a GUI dependency
to the language. Cross-language test data belongs in
[`fixtures/host-semantics/`](../fixtures/host-semantics/README.md).

All workspaces use the repository's ignored `target/` with `--profile lean`.
Build and acceptance commands are in the linked host guides and
[CONTRIBUTING.md](../CONTRIBUTING.md). Native desktop checks remain explicit;
portable tests do not claim a successful GUI interaction.
