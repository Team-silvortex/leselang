# Independent Consumers

These applications exercise public LeseLang interfaces from outside the main
language workspace. Their independence checks dependency and API compatibility;
all workspaces still share the repository's ignored `target/` cache.

## Choose An Example

| Goal | Entry |
| --- | --- |
| Embed from an independent Rust application | `examples/embedded_counter.rs` and the [embedding guide](../docs/embedding.md) |
| Discover a host catalog, generate one task and verify one execution | `examples/catalog_agent.rs` and the [Rust catalog-agent guide](../docs/agent-workflow.md) |
| Expose JS application objects and drive the real Rust VM | [`javascript/`](javascript) and the [Node consumer guide](../docs/javascript-sdk.md#run-the-independent-javascript-consumer) |
| Keep a JS application alive across agent tasks and launch Rust workers | [`javascript/agent.mjs`](javascript/agent.mjs) and the [Node-owned agent guide](../docs/node-agent-host.md) |
| Discover application-declared operations and verify two Effects | [Stock Consumer](javascript/agent/operations.mjs) and the [JS/TS application operations guide](../docs/application-operations.md) |
| Check the same program across Rust, Node and C# | [.NET Consumer](dotnet/Program.cs) and the [three-host contract](../docs/dotnet-host.md#compare-the-three-hosts) |
| Handle real worker Effects in Python, C or C++ | [Independent scalar Consumers](../docs/operations-host-protocol.md#python-c-and-c-consumers) |

The Rust catalog agent makes no model call and invokes a selected host as a
subprocess. It is distinct from the JavaScript SDK's
[multi-step workflow session](../docs/agent-workflows.md), which coordinates
several ordinary host sessions inside an application.

## Source Map

| Location | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | Separate Rust workspace consuming the five language crates |
| `tests/` | Public API, ownership, Fuel and repository-boundary acceptance |
| `examples/` | Rust embedding examples and the catalog agent |
| `examples/catalog_agent/plan.rs`, `plan/` | Internal plan facade, JSON/catalog admission, legacy strategies and tests |
| `examples/catalog_agent/semantic/` | Semantic admission, canonical source, report verification and versioned tests |
| `examples/catalog_agent/calculator.rs` | Retained Calculator compatibility strategy |
| `javascript/` | Original Rust-launched Node application and its public API acceptance suite |
| `javascript/agent/`, `javascript/agent.mjs` | Node-owned model, semantic tasks and offline workflow using the public Node transport |

Shared catalogs and generated-program goldens live in
[`fixtures/host-semantics/`](../fixtures/host-semantics/README.md). Consumers,
reference hosts and SDK tests compare the same fixtures. Application-specific
Node state and publication semantics remain in `javascript/`.

## Check Compatibility

From the repository root, with pinned dependencies installed:

```sh
sh scripts/check.sh consumer
sh scripts/check.sh javascript
sh scripts/check.sh registered
sh scripts/check.sh node-consumer
```

The Rust group includes the catalog-agent example's unit tests explicitly;
ordinary `cargo test --manifest-path consumer/Cargo.toml` does not execute those
example tests. The JavaScript group builds/verifies the SDK and Tauri frontend;
the Node group then executes the independent application through the real VM.
The [check guide](../scripts/README.md) gives exact prerequisites and commands.
None of these groups launches a GUI or contacts an external model service.
