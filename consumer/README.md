# Independent Consumers

These applications exercise public LeseLang interfaces from outside the main
language workspace. Their independence is a compatibility check, not a reason
to create separate build caches.

| Location | Responsibility |
| --- | --- |
| `Cargo.toml`, `Cargo.lock` | Separate Rust workspace consuming the five language crates |
| `tests/` | Public API, ownership, Fuel and repository-boundary acceptance |
| `examples/` | Rust embedding examples and the catalog-driven agent |
| `examples/catalog_agent/` | Catalog admission, source generation, verification and process execution |
| [`javascript/`](javascript) | Separate Node application importing the public JavaScript package |

Shared catalogs and generated-program goldens live in
[`fixtures/host-semantics/`](../fixtures/host-semantics/README.md). They are read
by consumers, reference hosts and SDK tests; they are not private agent assets.
Application-specific Node state and publication semantics remain in
`javascript/`.

Run from the repository root after fetching pinned dependencies:

```sh
cargo test --manifest-path consumer/Cargo.toml --locked --offline --profile lean
cargo test --manifest-path consumer/Cargo.toml --example catalog_agent --locked --offline --profile lean
```

The catalog agent example's own tests require the second command. Its
[guide](../docs/agent-workflow.md) explains planning and explicit execution.
The [JavaScript SDK guide](../docs/javascript-sdk.md#run-the-independent-javascript-consumer)
lists the separate Node build and acceptance commands.
