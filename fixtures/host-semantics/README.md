# Shared Host Contract Fixtures

These catalogs and generated LeseLang programs are shared contract data for the
Rust catalog agent, JavaScript SDK and reference hosts. They live outside any
one consumer or implementation so tests can compare the same bytes across
languages. Production host metadata is generated from actual registrations and
host-owned declarations; these files do not register operations or grant access.

| Files | Purpose |
| --- | --- |
| `appkit-v1.json`, `calculator-v1.json`, `calculator.lese` | Legacy discovery and planner compatibility without semantic metadata |
| `appkit-semantics-v1.json`, `tauri-semantics-v1.json`, `calculator-semantics-v1.json` | Current one-shot reference host declarations |
| `form-semantics.lese`, `calculator-semantics.lese` | Source generated from those declarations by the shared semantic vocabulary |
| `tauri-agent-semantics-v1.json` | The separate in-app Tauri task catalog |
| `tauri-save-message.lese`, `tauri-inspect-controls.lese`, `tauri-check-controls.lese` | Generated programs for named-input and no-input Tauri tasks |

Catalog JSON is an exact snapshot, including integer spelling and the final
newline. Generated source is compared byte for byte by the Rust and JavaScript
planner tests, then compiled and executed by host tests. Directory moves must
preserve those bytes.

When intentionally changing a contract:

1. Change its declaration under [hosts/shared](../../hosts/shared/README.md), or
   the Tauri-specific [agent declarations](../../hosts/tauri/src/agent_semantics.rs).
2. Read the rebuilt host's offline catalog. One-shot hosts expose `--describe`;
   Calculator also needs `--calculator`. The in-app Tauri catalog uses
   `--describe-agent`. Preserve the legacy fixtures when their compatibility
   contract has not changed.
3. Generate the affected source through the public Rust catalog agent or
   JavaScript `prepareSemanticCatalog` API. User task text remains an argument,
   never substituted into the source.
4. Review the catalog and source differences together. Run the catalog agent
   example tests, JavaScript SDK tests and the affected host tests as described
   in [CONTRIBUTING](../../CONTRIBUTING.md). Native GUI acceptance is separate.

Do not update expected files merely to suppress a failing compatibility test.
Runtime reports, logs, generated executables and desktop bundles belong under
the shared `target/` build directory, not here.
