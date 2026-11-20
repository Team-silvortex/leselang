# From Host Discovery To A Verified GUI Task

The independent [`catalog_agent` Rust example](../consumer/examples/catalog_agent.rs)
reads a reference host's JSON catalog, generates a LeseLang task, executes that
task once and checks its result against live control state. The same agent binary
supports AppKit and TypeScript forms, plus the real macOS Calculator application:

| Discovered profile | Task | Verified workflow |
| --- | --- | --- |
| `appkit` | `fill_and_save` | Find the input, replace its text, click Save and verify the captured value. |
| `tauri.typescript` | `fill_and_save` | Perform the same form task through the TypeScript host and verify DOM/application-model state. |
| `macos_ax.calculator` | `calculate_one_plus_two` | Find Calculator, observe its state, press `one`, `add`, `two`, `equals`, then verify the display and expression. |

These reference hosts now publish optional [semantic task declarations](host-semantics.md)
alongside their type catalogs. The deterministic agent validates the declarations,
generates their finite steps and evaluates their named verification predicates.
This path accepts new profiles, operation names, control names and task names
without an application-specific planner branch. The task must be offered by
the actual catalog; `--task` cannot override that contract. Type signatures
alone do not describe application meaning, and the agent makes no LLM call.
Older catalogs without semantic metadata retain the original strategies for
the three profiles above. Other AX profiles currently publish only their type
catalogs and have no legacy strategy in this example.

The semantic reader dispatches explicitly on versions 1 and 2. Version 1's fixed
results remain unchanged; version 2 additionally supports
[observed result contracts](host-semantics.md#version-2-observed-results). Such a
contract compares the actual typed result with a declared actual report path,
then retains the original report predicates and bounds. It does not accept
an arbitrary value merely because its type matches. Unsupported versions still
fail without falling back to a legacy profile strategy.

The example belongs to the separate Consumer workspace. It depends on the five
language crates and their existing JSON dependencies, and invokes the selected
host as a subprocess. It has no AppKit/Tauri/Accessibility dependency or GUI
handle. A model-based planner can use the same discovery and execution
boundaries, while the application owns operation meaning and declared
postconditions and the planner owns its task choice and generated source.

## Build And Inspect A Plan

Run these commands from the repository root after fetching the pinned
dependencies:

```sh
cargo build --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean
cargo build --manifest-path consumer/Cargo.toml --example catalog_agent --locked --offline --profile lean
target/lean/examples/catalog_agent \
  --host target/lean/leselang-appkit-host \
  --output /tmp/leselang-agent-plan \
  --text '保存 ✓'
```

The commands above select AppKit. After building the
[Tauri host](typescript-host.md#build-and-run), use that same agent binary for a
TypeScript form plan:

```sh
target/lean/examples/catalog_agent \
  --host target/lean/leselang-tauri-host \
  --output /tmp/leselang-agent-tauri-plan \
  --task fill_and_save --text '保存 ✓'
```

Calculator uses an explicit host selector and a different task, with no text
input:

```sh
cargo build --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean
target/lean/examples/catalog_agent \
  --host target/lean/leselang-macos-ax-host \
  --host-arg --calculator \
  --output /tmp/leselang-agent-calculator-plan \
  --task calculate_one_plus_two
```

`--host-arg` consumes one literal argument. Here it forwards `--calculator` to
both the offline `--describe` invocation and the later `--run` invocation.
Repeated selectors retain their order. The selected task declaration supplies
the steps, native report contract and bounded host argument convention. For
legacy catalogs, the actual profile chooses the original strategy. Selecting the
Calculator task against a form catalog, or the form task against Calculator,
fails during planning before source generation or execution.

Choose an unused output path. The agent creates a new directory and refuses an
existing one, including a directory containing an earlier plan. It preserves
the catalog, generated source, task inputs and verification outcome as separate
artifacts. Form text is passed as a typed `main(text: string)` input; quotes,
backslashes and Unicode in it are data, never interpolated into source.
The Calculator program has a parameterless `main()` and the fixed task
`1 + 2 = 3`.

The default mode only runs the reference host's offline `--describe`, validates
the catalog and writes a plan. It creates no GUI or VM session. This path also
works on non-macOS systems with the portable host catalog binaries. A successful
plan establishes metadata compatibility, not the presence, state or authority
of live controls.

The `--host` and `--output` options are required. The task defaults to
`fill_and_save`; an explicit `--task` accepts a bounded identifier, with its
existence and inputs checked against the catalog during planning. The reference
forms require `--text`; Calculator uses `calculate_one_plus_two` and rejects
`--text`. A declared task can name its single string input differently while
the CLI still supplies it as `--text`. The host path selects
the executable directly; it is not a shell command. The selected executable
remains trusted host code. Its JSON output is neither an authenticated claim
about another process nor a grant of GUI access.
Task text cannot contain NUL because it crosses the process argument boundary.

## Execute And Verify Once

On macOS 14+ with Xcode Command Line Tools and a logged-in desktop session, use
a different unused output directory and add `--execute`:

```sh
target/lean/examples/catalog_agent \
  --host target/lean/leselang-appkit-host \
  --output /tmp/leselang-agent-run \
  --text '保存 ✓' \
  --execute
```

The agent reads and validates a fresh catalog for this invocation. It then
launches the host once with the generated file and `--report-json`. For forms,
it supplies the original text through the selected host's typed `--text`
arguments. The form host creates its own disposable AppKit or WebView form; it
does not select an unrelated application's window. The agent does not request `--hold`,
so the window closes when the host exits. AppKit rejects `--hold` with JSON
reporting; the Tauri host permits it for a separate manual inspection run.

The successful form source path has nine Effects:

1. List the registered control identifiers.
2. Find `input` and observe its role, editability, enabled state and revision.
3. Submit text replacement using that observed revision, then observe the value.
4. Find `save` and observe its role, enabled state and revision.
5. Submit its click using the new revision, then observe the captured value and
   target/action count.

The generated form program returns a Boolean postcondition. A submission
acknowledgement alone is insufficient: the edited value, saved value and Save
callback count must match after fresh observations. Failed guards return
`false` or terminate with a host error; they do not select a fallback action.

After form execution, the agent independently checks the structured report. It
requires the program's Boolean result to be `true`, the expected nine Effects,
the intended text in both the input and Save, exactly one new Save callback,
and unchanged reported scalar state for `other`. It validates the report's
shape and version; a missing, malformed or contradictory report cannot become
a successful task.

### Calculator In An Existing Application

Prepare the [supported Calculator window](macos-ax-host.md#calculator): the
application must already be running in Basic mode, with unit conversion off,
the supported English button labels and a display of exactly `0`. Arrange
Accessibility permission for the host process and avoid interacting with that
window during the run. Then execute using another unused evidence directory:

```sh
target/lean/examples/catalog_agent \
  --host target/lean/leselang-macos-ax-host \
  --host-arg --calculator \
  --output /tmp/leselang-agent-calculator-run \
  --task calculate_one_plus_two \
  --execute
```

This command affects the existing Calculator window and any calculation history
the application retains. It does not launch a disposable form, start or close
Calculator, clear its display or restore its previous state. Prepare a fresh
initial state explicitly before deciding to run another calculation.

The generated source finds Calculator and observes its initial display before
issuing a press. A display other than `0` returns `"initial display mismatch"`
after two read Effects, with no presses. The success path performs ten Effects:
one find, five observations and four presses. Each press uses the revision from
the immediately preceding observation. The program checks all four press
acknowledgements and requires the final display `3` **and** last expression
`1 + 2`; a displayed `3` from another expression is insufficient.

The agent separately requires the host's typed string result `"3"`, ten
Effects, remaining Fuel within the original 2,000 budget, `verified: true`, and
fresh native before/after evidence. Those snapshots must establish initial
display `0`, final display `3` and expression `1 + 2`, along with exactly the
four admitted, enabled buttons. Missing, duplicated, disabled or different
buttons fail verification. A prior `last_expression` may be a string or `null`;
it does not replace the initial display guard. The Calculator report cannot
satisfy a form task, nor can a form report satisfy this task.

All three workflows retain the host's explicit
`poll → authorize → dispatch → pump → accept` sequence. Action pumps use the
original VM Fuel. No catalog field, task selector or successful planning step
replaces live role, revision, object identity or authority checks.

## Evidence Files

The output directory separates inputs, source and runtime evidence:

| File | Content |
| --- | --- |
| `catalog.json` | The actual host catalog read for this invocation. |
| `discovery.stderr` | Standard error captured from offline discovery. |
| `program.lese` | Generated task source, independent of the supplied text. |
| `plan.json` | Selected profile and task, typed inputs, literal host arguments, expected Effects, required capabilities and `planning_contract` (`catalog_semantics_v1` or `legacy_profile`). |
| `run.json` | The host's structured execution output, when execution was attempted. |
| `run.stderr` | The execution trace or failure diagnostics. |
| `verification.json` | The final phase, status and possible-partial-effects classification. |

Planning success and verified execution are different outcomes. Inspect
`verification.json` together with the source and execution report; a directory
or a generated program does not establish that any action ran. Files produced
before an error remain available for diagnosis. They can contain the task text
and observed values.

All execution reports have `schema_version: 1` and profile-specific identities:

| Catalog `profile` | Report `kind` | Success Effects | Initial VM Fuel |
| --- | --- | --- | --- |
| `appkit` | `appkit_run` | 9 | 1,000 |
| `tauri.typescript` | `tauri_run` | 9 | 10,000 |
| `macos_ax.calculator` | `macos_ax_calculator_run` | 10 | 2,000 |

Each includes a typed scalar `result`, `effects`, `fuel_remaining`, and
`native_before` / `native_after` arrays. TypeScript and Calculator reports must
additionally have `verified: true`; a value from another profile cannot satisfy
this check. The form arrays contain the public scalar fields
`identifier`, `role`, `title`, `value`,
`editable`, `enabled` and `clicks` for the three controls. They contain no native
pointer, object reference, lease or execution identity. In the TypeScript host,
the same array field names describe public DOM/application-model state.

Calculator arrays each contain exactly one public snapshot with
`identifier: "calculator"`, `role: "AXWindow"`, `display`, `last_expression`
and `buttons`. Each button is `{ "name": "one", "enabled": true }` with the
corresponding admitted name. `last_expression` is a required nullable string;
omitting it is not equivalent to `null`. Button order does not confer meaning,
but names must form the exact unique set `one`, `add`, `two`, `equals`.
No PID, native window or button identity is exported. The host independently
checks that before, program observations and after refer to the same native
instance.

The before/after snapshots are additional host verification reads around the
run. They are separate from the program's nine or ten Effects and are not
charged to its VM Fuel. Their purpose is to cross-check the program result
against the real AppKit, WebView or Calculator controls. They are local
evidence from the selected host, not a transport protocol, reusable object
capability or atomic GUI transaction.
Object identity and replacement checks remain inside the host; the scalar
report cannot establish that two observations refer to the same native instance.

## Bounds And Failure Handling

| Input or subprocess boundary | Limit |
| --- | --- |
| Catalog output | 256 KiB |
| Generated LeseLang source | 16 KiB |
| Task text | 4,096 UTF-8 bytes, with no NUL |
| Literal `--host-arg` selectors | At most 8 arguments, each 1–1,024 UTF-8 bytes, with no NUL |
| Execution report | 128 KiB |
| Standard error from each host invocation | 128 KiB |
| Each host invocation | 30 seconds of wall time |

A selector cannot be the argument terminator `--` or one of the agent-owned
execution flags: `--describe`, `--run`, `--entry`, `--text`, `--report-json`, `--hold` and
`--help`, including their `=...` forms. The selected host still validates its
own options. These literal arguments are never passed through a shell and do
not grant access to an application or authorize a different task.

Each host retains the fixed Fuel budget shown above. Text admission and Fuel
are independent: a string can fit the input bound while requiring more Fuel
than the full workflow has available. Fuel exhaustion remains a terminal
failure and can happen after an earlier action. The agent never raises the
budget or reruns a task to obtain a successful result.

Unsupported catalog or semantic versions, unsupported legacy profiles, missing operations, incompatible
parameters, results or fields, malformed JSON and oversized output stop the
workflow. Output limits and wall time are external process bounds, not a
language scheduler or a substitute for Fuel. If execution has started, an
error, timeout, nonzero exit, partial report or failed postcondition is treated
as potentially having delivered side effects. Killing or cancelling the
process cannot undo a text replacement, callback or Calculator press already
performed.

There is no automatic retry, repair prompt, rollback or replay. Inspect the
saved evidence and current state before deciding whether another invocation
is appropriate. Calculator is never reset automatically, even after a failed
initial-state guard or a mismatched result. A new invocation creates a new
execution; form hosts create new forms, while Calculator remains the existing
application with its current state. Neither path resumes the previous session.
Present but invalid semantic metadata never falls back to a legacy strategy.
The generic verifier checks the selected host's declared predicates; these
must be sufficient for the application's task. A valid declaration is not a
proof of its author's claims and does not grant runtime authority.

## Recorded Desktop Verification

The dated evidence is preserved in [Independent Catalog Agent](acceptance/agents.md#independent-catalog-agent).

### Same Agent Across Tauri And Calculator

The dated evidence is preserved in [Same Agent Across Tauri And Calculator](acceptance/agents.md#same-agent-across-tauri-and-calculator).

### Catalog-Declared Tasks

The dated evidence is preserved in [Catalog-Declared Tasks](acceptance/agents.md#catalog-declared-tasks).

## Verify Changes To The Workflow

Run the example's portable planner, verifier and process-boundary tests:

```sh
cargo test --manifest-path consumer/Cargo.toml --example catalog_agent --locked --offline --profile lean
cargo clippy --manifest-path consumer/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
```

CI is configured to exercise these tests and offline plan generation on Linux
and macOS. The GUI command above remains an explicit desktop acceptance check;
offline success does not establish native GUI behavior. Keep the full
[Consumer suite](../CONTRIBUTING.md#verification) and the
[AppKit conformance checks](appkit-host.md#verification-and-scope) passing when
changing shared behavior.

See the [catalog schema](host-discovery.md) for metadata compatibility and the
[embedding guide](embedding.md) for explicit session and Effect driving.
