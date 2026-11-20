# Reference Hosts

Each adapter owns its operation meaning, application objects, permissions and
event processing. The language crates do not depend on these workspaces.
Choose AppKit for application-owned native controls, Tauri/the JavaScript SDK for
objects registered by a JS/TS application, and Accessibility for explicit control
of another macOS application's selected window. The AX adapter requires its own
permission and layout evidence; application registration is a different boundary.

| Directory | Scope | Guide |
| --- | --- | --- |
| [`appkit/`](appkit) | In-process macOS controls and a main-thread native bridge | [AppKit](../docs/appkit-host.md) |
| [`macos-ax/`](macos-ax) | Explicitly scoped cross-process Accessibility control | [macOS AX](../docs/macos-ax-host.md) |
| [`tauri/`](tauri/README.md) | Registered JS objects, a Rust VM owner and desktop WebView transport | [TypeScript host](../docs/typescript-host.md) |
| [`registered/`](registered/README.md) | Portable registered-object library and standalone VM worker shared with Tauri | [Node host](../docs/node-agent-host.md) |
| [`shared/`](shared/README.md) | Source-only reference application semantic declarations | [Host semantics](../docs/host-semantics.md) |

The adapters and registered-object core are independent Cargo workspaces with
their own manifests and lockfiles. Tauri depends on the portable core; the core
does not depend on Tauri. `shared/` is not another crate or workspace: it holds host-owned
declarations reused by the reference adapters without adding a GUI dependency
to the language. Cross-language test data belongs in
[`fixtures/host-semantics/`](../fixtures/host-semantics/README.md).

All workspaces use the repository's ignored `target/` with `--profile lean`.
[Shared check groups](../scripts/README.md) cover portable, build and native
boundary checks. Use the linked host guides for setup and explicit desktop
procedures, and [CONTRIBUTING.md](../CONTRIBUTING.md) for changed-layer requirements.
Dated [acceptance records](../docs/acceptance/README.md) are separate from current
instructions; portable tests do not claim a successful GUI interaction.
