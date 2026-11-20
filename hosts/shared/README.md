# Shared Reference Host Declarations

[`semantics.rs`](semantics.rs) contains host-authored task declarations shared
by the AppKit, macOS Accessibility and Tauri reference adapters. Each independent
host includes this source as an internal Rust module; it is not a language crate
or a new Cargo workspace.

The declarations describe application operation meaning, finite workflow steps
and report predicates. They do not implement native dispatch, grant capabilities
or alter VM execution. Actual operation and type registrations remain with each
host and validate the declarations before catalog output.

Keep reusable reference declarations here. Application-specific additions such
as the Tauri conversation tasks stay with their host, in
[`tauri/src/agent_semantics.rs`](../tauri/src/agent_semantics.rs). Keep generic
schema validation in `leselang-runtime-core` and planner implementations in their
respective Rust and JavaScript consumers.

The cross-language expected catalogs and generated programs are under
[`fixtures/host-semantics`](../../fixtures/host-semantics/README.md). See the
[semantic contract guide](../../docs/host-semantics.md) for the schema and its
limits. Changes to this shared source require checks for every host that
includes it; host-specific framework dependencies must not enter this module.
