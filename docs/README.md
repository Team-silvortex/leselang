# Documentation

Choose a path by what you are building. These guides describe the current
executable contracts; recorded desktop checks establish the tested application
and platform scope.

## Start Here

| Goal | Guide |
| --- | --- |
| Write and run LeseLang | [Language and CLI](language.md) |
| Embed the compiler and VM in Rust | [Rust embedding](embedding.md) |
| Expose JavaScript or TypeScript application objects | [JavaScript SDK](javascript-sdk.md) |
| Add an application-owned agent conversation | [Agent entry](agent-entry.md) |
| Connect natural-language planning to a model service | [Model provider](model-provider.md) |
| Execute several declared tasks from one request | [Sequential agent workflows](agent-workflows.md) |
| Understand dependencies and ownership | [Architecture](architecture.md) |
| Work on the repository | [Contributor checks and build output](../CONTRIBUTING.md) |

## Catalogs And Agent Workflows

- [Host protocol](host-protocol.md): observation, actions, identities and authority.
- [Catalog discovery](host-discovery.md): versioned operation and type descriptions.
- [Host semantics](host-semantics.md): finite tasks and declared verification conditions.
- [Model provider](model-provider.md): natural-language task selection, native credentials and verified execution.
- [Sequential agent workflows](agent-workflows.md): complete-plan admission, per-step verification and retained failure evidence.
- [Independent catalog agent](agent-workflow.md): discover, generate, execute and verify.
- [Shared compatibility fixtures](../fixtures/host-semantics/README.md): catalog bytes
  and generated source consumed by Rust and JavaScript tests.

## Reference Hosts

| Host | Entry guide |
| --- | --- |
| Application-owned native controls | [AppKit](appkit-host.md) |
| Registered TypeScript controls in a WebView | [Tauri](typescript-host.md) |
| Scoped control of another macOS application | [Accessibility](macos-ax-host.md) |

The [host source map](../hosts/README.md) identifies each independent workspace.
For macOS Accessibility integration, continue with the relevant operation family:

- [Application and window selection](macos-ax-applications.md)
- [Control discovery and actions](macos-ax-controls.md)
- [Menus, checkboxes, lists and scrolling](macos-ax-widgets.md)
- [Explicit auxiliary-process grants](macos-ax-auxiliary-processes.md)
- [Host-driven observation waits](macos-ax-observation-wait.md)
- [Source-declared value waits](macos-ax-source-waits.md)
- [Source-declared checked, selection and scroll waits](macos-ax-state-waits.md)

## History And Provenance

[Repository handoff](migration.md) records the product extraction and release
gates. The [extraction baseline](provenance/extraction-baseline.md),
[runtime-core baseline](provenance/runtime-core-baseline.md),
[source checkpoint](provenance/source-checkpoint.json) and
[bootstrap verification](provenance/bootstrap-verification.json) are historical
evidence, not additional runtime dependencies.
