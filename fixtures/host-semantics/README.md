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
| `tauri-agent-semantics-v1.json` | Original four-task Tauri catalog with fixed result contracts; retained unchanged |
| `tauri-agent-semantics-v2.json` | Opt-in five-task Tauri catalog with the observed-result `read_message` task |
| `tauri-save-message.lese`, `tauri-inspect-controls.lese`, `tauri-check-controls.lese` | Generated programs for named-input and no-input Tauri tasks |
| `tauri-read-message.lese` | Generated no-input read task returning the current observed input string |
| `tauri-board-semantics-v2.json` | Separate task board declarations under `tauri.taskboard` |
| `tauri-board-{read-review,read-archive,complete-review,complete-archive,check}.lese` | Five no-input board programs, compiled by Rust and byte-compared by the SDK |
| `javascript-agent-semantics-v2.json` | Node publication tasks expanded by the public registered-task constructor, using the standalone worker's actual base catalog |
| `javascript-{read-draft,publish-article,read-publication,check-publication}.lese` | Four expanded programs compared by Rust and JavaScript and executed by the independent Node Consumer |

Catalog JSON is an exact snapshot, including integer spelling and the final
newline. Generated source is compared byte for byte by the Rust and JavaScript
planner tests, then compiled and executed by host tests. Directory moves must
preserve those bytes.

When intentionally changing a contract:

1. Change its declaration under [hosts/shared](../../hosts/shared/README.md), or
   the Tauri-specific [agent declarations](../../hosts/tauri/src/agent_semantics.rs)
   or [task board declarations](../../hosts/tauri/src/board_semantics.rs). The
   JavaScript publication declarations live in the independent Consumer's
   [agent catalog](../../consumer/javascript/agent/catalog.mjs).
2. Read the rebuilt host's offline catalog. One-shot hosts expose `--describe`;
   Calculator also needs `--calculator`. The in-app Tauri catalog uses
   `--describe-agent` for the original four-task v1 contract and
   `--describe-agent-observed` for the opt-in five-task v2 contract. The board
   uses `--describe-agent-board` with its separate application profile. Preserve
   legacy fixtures when their compatibility contract has not changed.
   For the JavaScript catalog, pass `leselang-agent-worker --describe` output to
   `createPublicationAgentCatalog` and append a final newline to its JSON.
3. Generate the affected source through the public Rust catalog agent or
   JavaScript `prepareSemanticCatalog` API. User task text remains an argument,
   never substituted into the source.
4. Review the catalog and source differences together. Run the catalog agent
   example tests, JavaScript SDK tests and the affected host tests as described
   in [CONTRIBUTING](../../CONTRIBUTING.md). Native GUI acceptance is separate.

Do not update expected files merely to suppress a failing compatibility test.
An observed result fixture declares the evidence path and scalar type, not an
expected application value. Test different execution values against the same
catalog rather than changing its golden bytes to match live state.
Runtime reports, logs, generated executables and desktop bundles belong under
the shared `target/` build directory, not here.
