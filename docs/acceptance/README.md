# Acceptance Records

These records preserve observed results from named development milestones.
Dates, application versions, source flows, Effect/Fuel counts and failure scope
belong to those runs. Historical test totals are not the current suite size.
Current setup and contract instructions remain in the linked guides.

| Record | Milestones and evidence |
| --- | --- |
| [Development tensor](development-tensor.md) | 2026-11-27 local Phase I catalog validation, task selection, dependency isolation and language and host regressions |
| [AppKit](appkit.md) | 2026-11-13 native form inspection, typed inputs and native helpers |
| [macOS Accessibility](macos-ax.md) | 2026-11-13 TextEdit, Calculator, Stickies, Finder, Preview and explicit wait checks |
| [JavaScript and Tauri](javascript.md) | 2026-11-13 independent Node Consumer and real Tauri form execution |
| [Agents](agents.md) | 2026-11-13 Rust catalog agent, semantic planning and conversation entry; 2026-11-20 workflows, observed results, conditions, two TypeScript GUIs, public registered/Node interfaces, installed Consumers, standalone Rust worker and typed semantic authoring. The 2026-11-27 records cover application-declared operations, the Windows worker, conditional selection, [shared operation failures](agents.md#2026-11-27--shared-operation-failure-vectors), [lost write replies](agents.md#2026-11-27--losing-a-write-request-or-its-successful-reply) and [same-key retry](agents.md#2026-11-27--same-key-retry-after-an-interrupted-write) across Rust, Node and C# Consumers. |

Each record distinguishes real GUI execution, an independent application
observation, deterministic test coverage and local model-protocol fixtures.
Predetermined planning replies do not establish online model inference or
natural-language interpretation quality. Paths under `target/` identify local,
ignored evidence rather than committed artifacts available to every checkout.

When adding a record, include the actual platform and application state, the
source or selection used, the executed Effects and observable result, failure
or cancellation behavior actually exercised, and any separate automated
coverage. Preserve older records when a new contract or test count supersedes
them. Do not replace a failed or partially completed run with a later success.

See [testing requirements](../testing.md) for current verification procedures
and [provenance](../migration.md) for the earlier repository extraction history.
