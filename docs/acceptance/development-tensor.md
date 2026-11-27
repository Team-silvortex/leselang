# Development tensor acceptance

On 2026-11-27 the v1 catalog and the standalone `leselang-dev` 0.1.0
tool were exercised on macOS arm64 with Rust 1.98.0. The local working
tree was based on `326c98969dc507ec55afdf14ba8a807eacb7f740` and included
uncommitted language and host changes. The SHA-256 of the catalog file inventory
was `a57a5b341023e6c06cef63a2695c84ea838435de8e6ea5b592ff2e5258930c6f`.
The hash input is one UTF-8 line per JSON file under `project/status/`, sorted
by repository-relative path, containing the lowercase file SHA-256, two spaces,
the path and LF.

`sh scripts/check.sh tensor` passed formatting, eight validator tests,
seven CLI integration tests and strict Clippy. Its native validator accepted
eight domains, 37 owned modules and 90 coordinates. Independent tree traversal
reproduced the portfolio and delivery aggregates. The five deferred system
integration cells had zero delivery weight. The evidence inventory contained
262 references and zero benchmark references. A repeated source anchor was
identified during calibration and replaced with a unique identity.

`tensor next --json` produced a ready card for
`hosts/conformance/shared-failure-vectors`. The expected artifact was a shared
denial and partial-write fixture with assertions in the three independent
Consumers. Test fixtures also exercised blocked and complete mainlines, drift
outside active filters and a genuinely isolated capability with a recorded
reason. Cargo resolved separate workspace graphs for the tool and language VM.

The existing language suite passed 2,413 tests and the separate Rust Consumer
group passed 850. The AppKit, macOS AX, JavaScript, registered worker, portable
Tauri, Node Consumer, .NET Consumer and smoke groups passed. The initial Tauri
desktop run encountered a sandbox refusal to bind its loopback HTTP fixtures.
The complete desktop group passed with local loopback access. Smoke generated
offline plans. This run used the noninteractive host checks, and historical
GUI and Windows receipts retain their own platform and revision.

For current results and reproduction commands, query the [development tensor](../../STATUS.md).

## 2026-11-27 — interpreting closure after the first mainline goal

After the three Consumers passed the shared failure vectors, the mainline had
one closed goal out of one. The same checkout contained 79 closed contracts
out of 90. In 27 of the 37 registered modules, every registered contract was
closed. The unfiltered portfolio mean was 88. The priority-weighted delivery
mean was 99 across 80 eligible contracts, with ten deferred targets carrying
zero weight.

The CLI regression covered a completed mainline with an open deferred
contract, an empty selection, and filters that selected only part of a module.
Module closure counts follow the matches in a summary. Task selection reports
the full mainline and catalog populations even under a filter.

## 2026-11-27 — turning a replay observation into the next task

All three Consumers could look up a receipt in a fresh session after a lost
write reply. All three also applied an ordinary write twice when the caller
resubmitted the same key. The [loss experiment](agents.md#2026-11-27--losing-a-write-request-or-its-successful-reply)
records both observations.

We added `hosts/conformance/write-reconciliation` as the second ordered goal.
Its reviewed status is `usable` at 50. Receipt lookup has three-Consumer
evidence. Safe resubmission for an operation offering same-key replay is
the outstanding part of the contract. After the catalog change, `tensor next`
selected that coordinate with no blocked prerequisites. It reported one
closed goal out of two and 79 closed contracts out of 91.

The [same-key retry run](agents.md#2026-11-27--same-key-retry-after-an-interrupted-write)
subsequently met the outstanding criterion. Rust, Node and C# each committed
once across the interrupted attempt and its retry. After a second key changed
the balance, the original key still returned its own receipt. The coordinate
moved to `stable` at 100 for settled writes in the same live application.
`tensor next` reported two closed goals out of two and 80 closed contracts
out of 91.
