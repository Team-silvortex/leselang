# Documentation

Choose a route by what you are building. Guides describe current executable
contracts. [Acceptance records](acceptance/README.md) preserve dated platform and
application evidence; they are separate from setup instructions.

## Start Here

| Goal | Recommended route |
| --- | --- |
| Write and run LeseLang | [Language and CLI](language.md) → [examples](../examples) |
| Embed the compiler and VM in Rust | [Rust embedding](embedding.md) → [host protocol](host-protocol.md) for native objects |
| Expose a JS/TS application's objects | [JavaScript SDK](javascript-sdk.md) → [independent Node consumer](../consumer/README.md) or [Tauri host](typescript-host.md) |
| Add an application-owned agent | [Agent entry](agent-entry.md) → [registered application host](registered-agent-host.md) → [JS sequential workflows](agent-workflows.md) → optional [model provider](model-provider.md) |
| Implement a GUI or CLI adapter | [Host protocol](host-protocol.md) → [catalog discovery](host-discovery.md) → [host semantics](host-semantics.md) → [reference hosts](../hosts/README.md) |
| Contribute to the repository | [Architecture](architecture.md) → [contributing](../CONTRIBUTING.md) → [check groups](../scripts/README.md) and [regression requirements](testing.md) |

## Catalogs And Agent Workflows

These references serve different layers:

| Guide | Contract |
| --- | --- |
| [Host protocol](host-protocol.md) | Objects, observation, action, identity and live authority |
| [Catalog discovery](host-discovery.md) | Versioned operation and type descriptions; no live grants |
| [Host semantics](host-semantics.md) | Finite tasks and report predicates; compatible v1 fixed results and opt-in v2 observed results |
| [Independent Rust catalog agent](agent-workflow.md) | A separate Consumer executable that discovers, generates, executes one task and verifies; no model call |
| [Application agent entry](agent-entry.md) | `AgentSession`, provider and application-host responsibilities |
| [Registered application agent host](registered-agent-host.md) | Reusable `createRegisteredAgentHost`, application connection, transport and independent observation checks |
| [Node-owned agent and Rust worker](node-agent-host.md) | A live independent JS application using the public host and optional Node transport with a fresh real VM worker per task |
| [Standalone registered-object core](../hosts/registered/README.md) | Framework-free Rust library and worker, shared with the Tauri compatibility layer |
| [JavaScript/TypeScript semantic authoring](semantic-authoring.md) | Public declaration types and validated composition of application task catalogs |
| [Registered GUI task declarations](registered-tasks.md) | Reusable read, set-text and click sequences with explicit application verification |
| [JavaScript sequential workflows](agent-workflows.md) | `AgentWorkflowSession`, complete-plan admission, result bindings and per-step verification |
| [Model provider](model-provider.md) | Natural-language task selection through an application-owned transport |
| [Shared fixtures](../fixtures/host-semantics/README.md) | Catalog bytes and generated source compared across Rust and JavaScript |

The Rust catalog-agent example and JavaScript workflow session are distinct
integration paths. They share catalog semantics and explicit verified execution;
the latter coordinates several ordinary host sessions from one admitted plan.

## Reference Hosts

| Host | Use it for |
| --- | --- |
| [AppKit](appkit-host.md) | Application-owned native macOS controls |
| [TypeScript through Tauri](typescript-host.md) | Registered JS objects, a Rust VM and a desktop WebView |
| [TypeScript task board](taskboard-host.md) | A second GUI sharing the same agent, with observed task status and one-time completion |
| [macOS Accessibility](macos-ax-host.md) | Explicitly scoped control of another application's window |

The [host source map](../hosts/README.md) identifies each independent workspace.
For Accessibility integration, continue with the relevant operation family:

- [Application and window selection](macos-ax-applications.md)
- [Control discovery and actions](macos-ax-controls.md)
- [Menus, checkboxes, lists and scrolling](macos-ax-widgets.md)
- [Explicit auxiliary-process grants](macos-ax-auxiliary-processes.md)
- [Host-driven observation waits](macos-ax-observation-wait.md)
- [Source-declared value waits](macos-ax-source-waits.md)
- [Source-declared checked, selection and scroll waits](macos-ax-state-waits.md)

## History And Provenance

[Acceptance records](acceptance/README.md) collect dated native desktop, Node and
agent checks with their original platform and evidence limits.
[Repository handoff](migration.md) records the product extraction and release
gates. The [provenance index](provenance/README.md) explains preserved source
references and the original snapshot scope. The [extraction baseline](provenance/extraction-baseline.md),
[runtime-core baseline](provenance/runtime-core-baseline.md),
[source checkpoint](provenance/source-checkpoint.json) and
[bootstrap verification](provenance/bootstrap-verification.json) are historical
evidence, not additional runtime dependencies.
