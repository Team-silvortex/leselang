# A Scoped macOS Accessibility Host

The independent [`hosts/macos-ax`](../hosts/macos-ax/Cargo.toml) workspace adapts
LeseLang to explicitly scoped applications in other processes. Its original TextEdit
profile edits one configured plain-text document. The Calculator profile
observes one Basic-mode window and presses a small set of its real buttons.
Both profiles find a native target, observe it, queue an operation and request
a separate observation to verify the result. They share the host's typed
session driver; the compiler, native object protocol, Fuel and VM remain the
existing Rust implementations.

This adapter uses macOS Accessibility (`AXUIElement`) through a private native
bridge. Each profile supplies its own operation catalog and native state while
using the same execution boundaries. Framework dependencies and target-specific
link flags stay outside the five language crates and the independent Consumer.
The existing TextEdit `Backend`, `Snapshot` and default driver entry points
remain available.

An additive [`--controls` mode](macos-ax-controls.md) discovers individual
buttons, text areas, text fields and static text within either selected window.
It provides connection-local keys, exact unique selectors and per-control
native references through the same driver. It requires explicit `--discover`
or `--run FILE`; the original profiles and demonstrations below remain the
default when `--controls` is absent. See the separate guide for its catalog,
10,000-Fuel budget and whole-scope freshness rules.

The [generic application/window interface](macos-ax-applications.md) supplies
another scope for that same controls catalog: an exact running `.app` bundle,
optionally narrowed by PID, and a uniquely selected window. `--app ... --windows`
reports metadata; explicit window selectors plus `--controls` discover or run
source in that pinned window. The original commands below retain their behavior.

## Prepare And Run

Requirements:

- macOS 14 or later and a logged-in desktop session; each application's supported
  UI profile is checked separately.
- Rust 1.98.0 and Xcode or Xcode Command Line Tools with a macOS 14+ SDK.
- Accessibility permission for the process running the host.
- For TextEdit, an existing plain-text document already open in the application,
  selected by its exact local file path.
- For the original Calculator profile, its Basic mode with unit conversion off
  and the supported English button labels. Other layouts or labels refuse rather
  than guessing.

Build from the repository root:

```sh
cargo build --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean
target/lean/leselang-macos-ax-host --check-permission
```

`--check-permission` reports whether the current process has Accessibility
access. It does not prompt, change system settings or start either application. If access
is unavailable, the command fails before GUI work. Permission granted to another
tool does not establish that this executable has access. Any required access
must be arranged explicitly in macOS before running the adapter again.

### TextEdit

Create a new disposable test document at a path that does not already exist:

```sh
target/lean/leselang-macos-ax-host --prepare /tmp/leselang-ax-demo.txt
```

`--prepare` uses exclusive file creation and never overwrites an existing file.
It writes the UTF-8 text `LeseLang AX initial` followed by a newline. Open that
exact file in TextEdit through the application's ordinary file-opening flow,
then run:

```sh
target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt
```

The default [LeseLang source](../hosts/macos-ax/examples/textedit.lese) checks
that the text area is editable and contains exactly the prepared initial text.
It then replaces the text with `LeseLang cross-process ✓` followed by a newline
and verifies the new value through a separate observation. The successful
source path performs four Effects: find, observe, set text and observe. A
second run against already changed content refuses the demo's initial-content
condition. Prepare and open a different new test file to repeat the demonstration.

The adapter does not send a save command. TextEdit owns document persistence and
may autosave edits, so the demonstration uses a dedicated test file. It does
not close the document or terminate the application after the run.

An independent source file can use the same scoped catalog:

```sh
target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt --run hosts/macos-ax/examples/textedit.lese
```

### Calculator

Open Calculator and select its Basic mode with unit conversion off. The default
demonstration requires the observed display to be exactly `0`; it does not clear
an existing calculation. The verified UI profile is Calculator 12.0 on macOS
27.0.1 with English button labels. The host's macOS 14 minimum build target does
not establish compatibility with every earlier Calculator layout. Then run:

```sh
target/lean/leselang-macos-ax-host --calculator
```

The [Calculator source](../hosts/macos-ax/examples/calculator.lese) finds the
scoped window, observes its state, presses `one`, `add`, `two` and `equals`, and
observes after each press. The final observation must contain display `3` and
last expression `1 + 2`. This source
path uses ten Effects: one find, five observations and four presses. Each press
uses the revision from the immediately preceding observation. Calculator owns
the calculation and any history it retains; the host does not clear or restore
application state afterward.

To run independent source against that profile:

```sh
target/lean/leselang-macos-ax-host --calculator --run hosts/macos-ax/examples/calculator.lese
```

For a bounded machine-readable report, add `--report-json`:

```sh
target/lean/leselang-macos-ax-host --calculator \
  --run hosts/macos-ax/examples/calculator.lese --report-json
```

This option is available only with the standalone Calculator profile. It may
also run the default demonstration when `--run` is omitted; it cannot be
combined with `--describe`, TextEdit, controls, widgets, task inputs or window
selectors. Offline discovery remains
`leselang-macos-ax-host --describe --calculator` and does not touch the GUI.

JSON reporting verifies the specific `1 + 2 = 3` workflow. Standard output
contains one complete JSON record followed by a newline, bounded to 128 KiB;
execution diagnostics go to standard error. The record has
`schema_version: 1`, `profile: "macos_ax.calculator"`,
`kind: "macos_ax_calculator_run"`, a typed string `result`, `effects`,
`fuel_remaining`, `verified` and `native_before` / `native_after` arrays.
Each array contains one public snapshot with `identifier: "calculator"`,
`role: "AXWindow"`, `display`, nullable `last_expression`, and the four allowed
buttons' `name` and `enabled` fields. Native object identities and process
identifiers remain private to the host.

The extra before/after native reads bracket the normal driver and are outside
the program's Effect count and VM Fuel. Successful verification requires an
initial display of `0`, a string result of `3`, ten Effects, final display `3`
and last expression `1 + 2`. The host also checks enabled buttons and the same
native instance across the initial, program and final observations. A matching
number alone is insufficient. A completed string-returning program with a
failed postcondition emits `verified: false` and exits unsuccessfully. An
execution or native-read failure produces no complete verification report;
an output failure may leave partial bytes. Neither case retries or undoes
earlier presses.

The independent [catalog agent](agent-workflow.md) consumes this report through
the same discovery, generation, execution and verification interface it uses
for AppKit and TypeScript forms. Select this host with
`--host-arg --calculator --task calculate_one_plus_two`, with no `--text`.
The agent validates the actual Calculator catalog before generating its source,
then independently checks the structured native evidence. The guide includes
commands for offline planning and explicit execution against the existing
Calculator window; no run automatically launches, clears or closes Calculator.

`--calculator` and `--document FILE` are mutually exclusive. Source files must
contain valid UTF-8 and are limited to 16 KiB (16,384 bytes). The TextEdit runner
starts with 1,000 Fuel; the Calculator runner starts with 2,000. Custom source
supplies its own checks and final scalar result. In the default text-output
mode, the runner does not impose the built-in demonstrations' initial or final
values; Calculator JSON reporting instead checks the reference workflow above.
A native object or observation cannot escape as the final result: project
scalar fields before returning.
Rejecting a native final result does not undo earlier edits or button presses.
Printed strings escape control characters and are intended for human inspection,
not as a wire format.

## Operations And Observations

| Profile | Operation | Inputs | Result |
| --- | --- | --- | --- |
| TextEdit | `gui.find` | `name: "document"` | An opaque object reference for the scoped text area, bound to this execution |
| TextEdit | `gui.observe` | `target`: that reference | A historical snapshot of the text area |
| TextEdit | `gui.set_text` | `target`, `expected_revision`, `text` | An acknowledgement that the replacement was queued |
| Calculator | `gui.find` | `name: "calculator"` | An opaque object reference for the scoped Calculator window, bound to this execution |
| Calculator | `gui.observe` | `target`: that reference | A historical snapshot of the display and permitted buttons |
| Calculator | `gui.press` | `target`, `expected_revision`, `button` | An acknowledgement that one named button press was queued |

The literal name `document` selects the host's explicitly configured document.
It does not select an arbitrary application, window or file. The CLI path is
part of host configuration, while the returned native object reference remains
necessary for observation and mutation.

TextEdit snapshots expose scalar `document`, `role`, `value`, `editable` and `revision`
fields. The document is the matched file URL; the role must be `AXTextArea`.
These values describe an observation. They do not grant authority, retain the
remote process or turn an old snapshot into current state. `gui.set_text`
replaces the complete text value through Accessibility; it does not synthesize
keyboard events or depend on which application has focus.

Calculator snapshots expose `display`, `last_expression`, `buttons`, `role`
and `revision`. `last_expression` is an `optional_string`: it contains the
expression exposed in Calculator's result view, or `none` when that view is
absent. The role is `AXWindow`; `buttons` is bounded scalar metadata listing the allowed
names `one`, `add`, `two` and `equals`. A button name has meaning only with the
live window capability and current host policy. Individual buttons are not
exported as separate native object references in this profile. The bridge
resolves and checks the actual button under the pinned window before invoking
its `AXPress` action. Display content, the optional last expression, and button
identities and enabled states are part of the expected native snapshot. An
observed change to any of these invalidates an older queued press. In particular,
a changed expression must invalidate the operation even if the displayed result
is unchanged; the next press can depend on that calculation context.

The [host catalog](../hosts/macos-ax/src/host.rs) and
[driver](../hosts/macos-ax/src/driver.rs) keep the handoff explicit:

1. `poll` yields a typed Effect request using the original session Fuel.
2. The host authorizes the current execution and checks live Accessibility access.
3. `dispatch` reads state or queues the profile's operation without delivering it.
4. For a queued operation, the driver explicitly calls `pump`. The host
   rechecks permission, object identity, observed state and the operation's
   prerequisites before entering the native setter or action.
5. The driver accepts the reply and polls again. Source requests a new
   observation to verify the resulting value.

There is no retry loop, background scheduler or implicit wait for the desired
value. A submission acknowledgement alone does not prove that the target
changed, and a successful native call does not replace the source's final observation.

## Process, Document And Object Scope

Each native connection is limited to its profile's system application:
`/System/Applications/TextEdit.app` with bundle identifier `com.apple.TextEdit`,
or `/System/Applications/Calculator.app` with identifier `com.apple.calculator`.
A bounded `libproc` scan examines at most 4,096 processes
owned by the current user and requires exactly one matching executable path.
It does not select by display name or foreground application. The connection
retains that executable path, PID and process start time, then checks them again
before accessing the target. A PID reused by a later process must not inherit
the old connection.

TextEdit window selection matches the canonical local file URL against
`AXDocument`. The bridge searches only within that matched window for the
editable text area. The original Calculator profile requires the unique main
window, the supported Basic/non-conversion mode, the anchored display and the four
allowed buttons. Unsupported or ambiguous layouts fail before an action.
Window and child lists are limited to 64 entries, and the scoped window's tree
walk is limited to 128 nodes and depth 16. Excess nodes, excessive depth,
ambiguous matches and unexpected native value types fail rather than selecting
the first convenient control. Empty child lists are checked before a bounded
array read, because AX can reject index zero on an empty collection. The host
also enforces the language's 4,096-byte
per-string bound before admitting snapshot text or a replacement value.

Retained window and control Accessibility references are checked against the
current scoped objects with `CFEqual`, together with process and profile
identity. A closed window, replaced element, different document or restarted
process invalidates the connection. The host does not silently rebind an old
LeseLang reference to a newly discovered object with similar visible text.

## Freshness Across Processes

The existing local object protocol is unchanged. Its revision still fences
observed state changes and consumes each admitted action attempt. The native
transport determines how much external history that revision can represent.

The application-owned AppKit host records every relevant mutation, including
changes from A to B and back to A. The target applications are separate processes, and this AX
adapter observes snapshots. A change that happens and is reversed entirely
between observations can go unseen. The bridge compares the latest observed
state immediately before entering the setter or action, but the other process can still
change between that read and the write. This is not an atomic remote
compare-and-set operation or a guarantee against all external ABA changes.

The same limitation applies to a snapshot's several Accessibility attributes:
they are bounded reads of remote state, not a transaction with the application.
Permission, process identity and element checks narrow the target and reject
detected changes; they cannot lock the external application's state. Source
must observe after the action and handle a mismatched result explicitly.

## Failures And Verification

Unavailable Accessibility access, a missing scoped target, stale identity,
uneditable text or disabled buttons, traversal limits and exhausted Fuel refuse
work before the native mutation is entered. Checks occur again when queued work
is pumped. Cancellation before delivery prevents the queued operation;
cancellation after delivery cannot undo an edit or button press already
performed by the application. Late replies remain subject to the
same execution and correlation checks as other native hosts.

Once the Accessibility setter or action has been entered, an error or timeout is an
uncertain outcome. The target application may have applied some or all of the
operation before the failure became visible. The host preserves that distinction and
does not automatically retry or claim rollback. A 0.5-second messaging timeout
is applied to each retained or traversed Accessibility reference. This bounds
an individual native message, not a whole traversal or execution. Fuel remains
a work budget and does not preempt the external application's execution.

Run the portable host/driver and command-line checks, then native build checks
on macOS:

```sh
cargo test --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean
cargo clippy --manifest-path hosts/macos-ax/Cargo.toml --all-targets --locked --offline --profile lean -- -D warnings
cargo fmt --manifest-path hosts/macos-ax/Cargo.toml --all -- --check
```

On macOS, also run the native boundary checks:

```sh
hosts/macos-ax/test-native.sh
```

These native boundary groups cover UTF-8 limits, canonical local document URLs, invalid
handles and output publication, AX error/empty-array handling, Calculator action
admission and expected-state dependencies, and the controls registry, metadata
and scope epoch. They make no remote Accessibility
calls and do not require or grant permission.

Deterministic backend tests establish the host's local queues, reply validation,
Fuel, cancellation and error classification. They do not establish that an
external application was read or changed. Real desktop acceptance requires the
configured TextEdit document or supported Calculator window, a successful
permission check, a run of the source and a separate check of the application's
resulting state. A denied
permission check or a successful compile is not a successful cross-process GUI
run.

CI runs this package's deterministic tests, formatting and strict Clippy on
Linux and macOS, plus the native boundary checks on macOS. The native AX
implementation is compiled on macOS; portable backend tests on Linux do not
execute the Apple frameworks. No CI test claims an authorized application desktop
session.

The 2026-11-13 local acceptance run used the dedicated
`target/ax-demo/LeseLang-AX-20261113.txt` document in the real TextEdit application.
The host found and observed `LeseLang AX initial\n`, explicitly delivered the
replacement, then observed `LeseLang cross-process ✓\n`. It completed four
Effects; the observation revision advanced from 0 to 3, and 651 Fuel remained
for that run. A separate desktop accessibility inspection confirmed the same
final text in the actual TextEdit window. The run used existing Accessibility
access without granting new system permission.

Two real negative checks also passed: selecting an existing but unopened test
file refused without falling back to the current window, and rerunning the
default demo against the already changed document stopped after its two read
Effects without issuing `gui.set_text`. A final external source-file run read
the same resulting text through two Effects, confirming that these refusal
paths left it intact.

The Calculator profile was separately verified on 2026-11-13 with Calculator
12.0 on macOS 27.0.1. An external run of `calculator.lese` started at display
`0`, pressed the four permitted buttons, and returned display `3` with
`last_expression` equal to `1 + 2`. It completed ten Effects with 1,656 Fuel
remaining; observation revisions progressed from 0 to 12. A separate desktop
accessibility inspection confirmed both the result and last expression in the
real Calculator window. The run used existing Accessibility access and did
not grant new system permission.

Rerunning the default Calculator demonstration at display `3` refused its
initial-state condition after two read Effects without pressing a button.
The existing TextEdit document also passed a two-Effect read-only source run
after the shared-driver refactor, returning its earlier verified text with
860 Fuel remaining.

Earlier on 2026-11-13, before controls mode was added, the host package passed
42 tests on macOS: 33 library tests, six
command-line tests, one external TextEdit API compatibility test and two
compile-fail ownership tests. Fourteen of the library tests exercise the
Calculator profile, including 14 before/after-queue dependency and authority
cases. The [external compatibility check](../hosts/macos-ax/tests/textedit_api.rs)
also preserves unannotated TextEdit
reply, native-value and empty run-summary construction alongside the existing
backend and snapshot interfaces. Strict Clippy, formatting checks and the six
pure native boundary groups passed. These results complement the real runs;
deterministic tests cover failure paths without manipulating other applications
or changing OS permissions.

The original profiles target a plain-text TextEdit document or the supported
Calculator Basic-mode window on macOS. The opt-in controls mode also accepts
an explicitly [selected application and window](macos-ax-applications.md),
using actual Accessibility metadata and the same bounded operation catalog.
Support for a particular application's layout still requires independent
verification. The optional [widgets catalog](macos-ax-widgets.md) covers scoped
popup menus, checkbox state, item selection and scrollbar positioning while
preserving these original profiles. Rich-text operations, application launching,
saving, Windows/Linux backends and a stable remote wire protocol remain
separate work. Local object references are never exposed as reusable remote handles.

Apple's [Accessibility API reference](https://developer.apple.com/documentation/applicationservices/axuielement)
describes the underlying calls. Its
[messaging timeout documentation](https://developer.apple.com/documentation/applicationservices/1459345-axuielementsetmessagingtimeout)
also distinguishes a per-element timeout from the current process's default.
