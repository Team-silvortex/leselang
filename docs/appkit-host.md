# A Real macOS AppKit Host

The standalone [`hosts/appkit`](../hosts/appkit/Cargo.toml) workspace connects
LeseLang to actual AppKit controls in its own application. It creates a native
window containing two `NSButton` objects, identified as `save` and `other`, and
an editable `NSTextField` identified as `input`. The form program discovers the
registered controls, observes the input, submits a text replacement, verifies
the edit, clicks Save and verifies the value captured by its real target/action.
The other button provides an independent target for routing and wrong-object checks.

AppKit is the first concrete framework because the development host is macOS
and its SDK already supplies native controls, object lifetimes and target/action
dispatch. A small Objective-C bridge calls those APIs; the compiler, object
protocol, Fuel and VM remain the existing Rust implementations. No GUI toolkit
dependency is added to the five language crates.

## Discover The Catalog Without A GUI

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --describe
```

`--describe` emits one JSON object followed by LF and returns before creating
an AppKit application, window, execution scope or session. It reads no source
file and invokes no GUI operation. The public Rust function
`leselang_appkit_host::describe()` also works on an ordinary background thread
without a `Gui` argument. The metadata API and command path are available on
non-macOS targets; actual GUI modes still require the platform and desktop
environment below.

The shared description schema has `schema_version: 1`, `protocol_version: 1`,
`catalog_version: 1` and `profile: "appkit"`. It describes all five operations,
their declaration-ordered named parameters, scalar or native types, results
and required capability labels. Native `object` is opaque; `snapshot` lists
the eight scalar fields admitted by the compiler. `gui.read` and `gui.write`
describe policy and grant no authority. A shared row builder supplies both
this description and the live catalog's independent result domains; compiler
type aliases and field mappings are also shared.
See the [shared discovery guide](host-discovery.md) for the common JSON schema,
encoding limits and compatibility rules.

The command accepts no GUI mode, `--hold`, source or task-input flags alongside
`--describe`. Success writes only JSON to stdout; failures use stderr. The
bounded shared `CatalogDescription::to_json()` encoder prepares a complete
record before writing. An output failure may leave a partial record and is
not retried. Discovery neither exports native handles nor establishes that a
particular control exists or is currently authorized.

## Build And Run

Requirements:

- macOS 14 or later with an active, logged-in desktop session.
- Rust 1.98.0, as pinned by the repository.
- Xcode or Xcode Command Line Tools providing `xcrun`, `clang` and a macOS 14+
  SDK. This reference adapter currently builds for the native host target.

Run from the repository root:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --demo --hold
```

The default demo runs the form workflow. `--hold` keeps its window available
until it is closed; omit that option for a finite run. The buttons use real
AppKit target/action callbacks, and `gui.set_text` replaces the real text field's
value. Save copies the input into its submitted value when its target/action
runs. A subsequent observation checks that captured value against the intended
text. A queued acknowledgement does not establish that either operation ran.

The original four-Effect button demonstration remains available:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --button-demo --hold
```

Run an independent source file against the same host:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --run hosts/appkit/examples/form.lese --hold
```

`--run` reads a UTF-8 source file of at most 16 KiB (16,384 bytes), before
constructing native GUI resources. It uses the same compiler, explicit driver
and operation catalog, with a fixed initial budget of 1,000 Fuel. It accepts
any scalar final result, including a string list, and does not impose the
demonstrations' button selection, expected value or state assertions. Returning
a session-bound native object or observation is an error; project its scalar
fields before returning. That final-result check does not undo earlier effects.
Printed scalar values are human-readable, with string controls escaped, rather
than a wire format. The selected host operations still act only on this
application's registered controls. The scalar `leselang` CLI does not register
these GUI operations.

For reusable tasks, `--entry NAME` selects a function and repeatable
`--text NAME VALUE` supplies string parameters without rewriting source. Both
flags require `--run`; `--text` alone selects `main`. Without either flag, the
existing parameterless `main` contract is preserved. The
[typed text task](../hosts/appkit/examples/text_task.lese) passes native `object`
and `snapshot` values through helpers, edits the selected text field and returns
`true` only after a fresh observation matches the input:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- \
  --run hosts/appkit/examples/text_task.lese --entry main \
  --text name input --text text 'LeseLang typed task ✓'
```

Task ingress accepts at most eight distinct input names and 4,096 UTF-8 bytes
per string. Missing, extra, duplicate or incorrectly typed inputs are rejected
before an Effect; `--text` performs no conversion to integer, boolean, optional
string or list. The native Rust `Host::compile_task` result exposes the compiled
node, inferred type and scalar entry parameters for callers using the shared
`embedding::bind_entry_arguments` validator and their own explicit VM loop.
Native helper types are host-owned declarations: a task cannot import an object
or snapshot from external input. The same driver preserves revision checks,
main-thread ownership, cancellation, Fuel and partial side effects.
The AppKit and Accessibility catalogs have different operation signatures;
the AX controls task can be reused across supported applications through its
[application-scoped adapter](macos-ax-controls.md#reuse-a-task-with-typed-inputs).

## Machine-Readable Execution Reports

For a caller that needs to verify a task result, combine `--run` with
`--report-json`:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- \
  --run hosts/appkit/examples/text_task.lese --entry main \
  --text name input --text text 'LeseLang typed task ✓' --report-json
```

On success, stdout contains one JSON object followed by LF. Execution traces
move to stderr. Without the flag, the existing human-readable output stays
unchanged. The flag requires `--run` and refuses `--hold`, demonstrations,
self-tests and catalog discovery, so its successful process exit is a finite
execution boundary.

The record has `schema_version: 1`, `kind: "appkit_run"` and `profile: "appkit"`.
`result` contains a scalar `type` and JSON `value`; the type distinguishes, for
example, `string` from `optional_string`. The record also reports `effects`,
`fuel_remaining`, `native_before` and `native_after`. Each native snapshot
projects only `identifier`, `role`, `title`, `value`, `editable`, `enabled` and
`clicks`. Native pointers, object references and leases are never serialized.

The before/after arrays come from additional native verification reads around
the run, separate from language Effects and their Fuel. A caller compares these
values against its own task postconditions. A successful process and a
submission acknowledgement do not by themselves mean that a task's intended
state was reached; a program can validly complete with `false`.

The host builds a complete report before writing, with a 128 KiB output bound.
Write or flush failure returns an error without retry; output or GUI effects
may already be partial. Runtime failure uses stderr and a nonzero exit, with no
success report. Reporting does not change the VM budget, live authorization,
explicit action pump or cancellation behavior.

The [independent catalog agent](agent-workflow.md) consumes this report after
generating its own nine-Effect form task from the validated catalog. It checks
the Boolean result, edited input, captured Save value, callback count and
unchanged public scalar state for `other`, and preserves the evidence for review.

## Native Desktop Checks

For a Finder-launchable local demo, run `hosts/appkit/bundle-demo.sh`. It builds
`target/LeseLang AppKit Host.app`; opening that bundle runs the form demo with
`--hold`. The bundle is a local development artifact, not an installed or signed
release.

Run the actual GUI acceptance checks separately:

```sh
cargo run --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean -- --self-test
hosts/appkit/test-native.sh
```

Both commands exercise AppKit on the process's main thread. The self-test drives
the complete Rust host and VM; the native bridge test checks framework edit
notifications, discovery bounds, UTF-8 ingress and lifecycle behavior directly.
Ordinary Rust test harness worker threads are not used for native GUI operations.
A machine without the required desktop session cannot supply this verification;
a successful compile is not a successful GUI run. Non-macOS execution reports
that the framework is unavailable.

Run the non-GUI command-line ingress tests and the compile-fail thread-affinity
check through the normal package test command:

```sh
cargo test --manifest-path hosts/appkit/Cargo.toml --locked --offline --profile lean
```

## Operations And Source Flow

The host's catalog supplies five operations:

| Operation | Inputs | Result |
| --- | --- | --- |
| `gui.list` | None | A bounded string list of registered control identifiers |
| `gui.find` | `name`: a control identifier | An opaque native object reference bound to this execution |
| `gui.observe` | `target`: that object reference | A historical observation with scalar state and identity-description fields |
| `gui.set_text` | `target`, `expected_revision`, and `text` | A submission acknowledgement for replacing an editable text field's value |
| `gui.click` | `target` and `expected_revision` | A submission acknowledgement for a button's target/action |

Discovery walks actual registered controls. The identifier list is ordinary
metadata, not a collection of object references or authority. A program can
inspect it with existing `len`, `item_at`, `value_or` and bounded `fold` operators,
then pass a chosen name to `gui.find`. Finding an object resolves its current
instance under live host policy. Retained discovery names do not keep a control
alive, freeze its state or authorize a future operation. Duplicate or invalid
identifiers must not silently select an arbitrary control.

An observation exposes these fields:

| Field | Meaning |
| --- | --- |
| `identifier` | The registered name observed for this control |
| `role` | The host's control role, used to distinguish buttons from text input |
| `title` | The native control's title or descriptive caption |
| `value` | Current input text for the text field; captured submitted text for Save |
| `enabled`, `editable` | Live state observed at capture time |
| `clicks` | The button's target/action count |
| `revision` | The local protocol revision associated with this observation |

These descriptions are scalar data. The separate native object reference is
still required by actions. The host rejects text replacement on a button and
clicks on the text field before native work. `gui.set_text` means replacement of
the whole value, not synthetic keyboard input, append, or focus manipulation.

The complete [form source](../hosts/appkit/examples/form.lese) makes the workflow
explicit: list identifiers, find the input, observe and check its role/editability,
submit and pump the replacement, observe the edited value, find and observe Save,
submit and pump its click, then observe the captured submitted value. The
[host catalog](../hosts/appkit/src/host.rs) and
[driver](../hosts/appkit/src/driver.rs) use the existing language implementation.
The original button program remains in the same host as a compatibility example.

## Thread, Identity And Event Boundaries

The GUI wrapper owns the native context and is neither `Send` nor `Sync`.
Creation, reads, mutations, event dispatch and destruction stay on the main
thread. Apple documents main-thread restrictions for views and cells, and the
main thread's responsibility for event delivery in its
[thread safety guide](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/Multithreading/ThreadSafetySummary/ThreadSafetySummary.html).

The bridge contains Objective-C exceptions at the FFI boundary. Rust receives
an error instead of an exception unwinding through a Rust frame. Native work
still remains trusted host code: Fuel meters the adapter's declared work and
does not preempt arbitrary framework callbacks.

Each host-owned object has a `LocalObjectOwner`. A replacement invalidates the
previous generation even if the new control uses the same identifier. The
bridge's per-instance identity is private metadata, not a serialized capability.
Changes made through the controls or bridge advance a checked mutation serial,
which is refreshed before revision-based admission. A title changed from A to B
and back to A still invalidates an earlier observation. This is an
application-owned adaptation obligation: every relevant
mutation path must participate in change tracking.

Save depends on the input's value. Input edits therefore also invalidate Save's
observed revision, including edits that change A to B and back to A. A queued
Save based on an older observation refuses after that dependency changes.
This is explicit dependency tracking for this form; it does not make multiple
objects a transaction or roll back an earlier accepted text replacement.

The VM handoff remains explicit:

1. `poll` produces a typed request.
2. The driver authorizes that request under current host policy.
3. `dispatch` either reads native state or queues the requested action.
4. For a submitted text replacement or click, the driver explicitly pumps queued
   work inside the same VM's metered host-work boundary. The pump rechecks the
   lease, object, generation, revision, control role and permissions before
   replacing text or invoking `performClick:`.
5. The driver explicitly accepts the reply, then polls for the next request.

The AppKit event step is also explicit. It requests at most one currently
available event through
[`nextEvent(matching:until:inMode:dequeue:)`](https://developer.apple.com/documentation/appkit/nsapplication/nextevent(matching:until:inmode:dequeue:)),
then dispatches that event. Framework event handlers may do native work; this
does not add a language scheduler or silently resume a pending VM.

Cancellation closes the language execution and prevents its queued action from
running. It cannot undo a text replacement or click already delivered to AppKit.
A later observation is the evidence for the resulting widget state. Rejected
replies preserve their native data and do not grant another object or execution
authority.

## Verification And Scope

The main-thread self-test exercises actual controls, including the original
button compatibility checks and the form workflow:

| Group | Evidence |
| --- | --- |
| Native controls and source routing | Registered identifiers resolve to distinct instances; source selects the intended control and leaves unrelated controls unchanged. |
| Typed task inputs and native helpers | The same source accepts different scalar inputs, passes native objects and snapshots through helpers, and verifies the actual edited value; malformed inputs and forged native types are rejected. |
| Submission, pump, acceptance and observation | Queue acknowledgement leaves native state unchanged; explicit pumps perform text replacement and target/action; new observations verify edited and submitted values. |
| Live state and authority | External changes, A→B→A changes, host-controlled and externally detected replacement, revocation and disabled/editability state are checked before native mutation. Save also tracks relevant input changes. |
| Cancellation and late replies | Cancellation prevents queued delivery; cancellation after delivery retains the real widget change and rejects the late reply. |
| Fuel | Zero initial Fuel or exhausting the original budget before pumping prevents native work; queued text replacement and clicks use the original session budget. |
| Exact reply identity | Another object's reference or snapshot payload is rejected while preserving the original reply, pending request and Fuel. |
| Partial effects | Injected known or uncertain failure after an actual native click preserves the GUI mutation and selects the corresponding VM terminal state. |
| Native boundaries | UTF-8 round-trips, oversized text refuses before mutation, and a worker-thread GUI constructor is rejected. |
| Teardown | Closing the real window refuses pending work and closes the execution. |

The retained button demo completes four Effects and returns `ready`; its button
changes from `draft`/zero clicks to `ready`/one click. The form demo completes nine
Effects and returns `LeseLang ✓`, after verifying the input edit and the value
copied by Save. The main-thread self-test covers 16 groups, including 32
stale-state, authority and dependency cases. The separate native bridge test
covers four groups, including real edit-notification delivery and bounded native
view traversal. Keep these state assertions separate from submission receipts.
Neither a returned acknowledgement nor a successful compiler invocation is
evidence that a control changed.

The local form run was also checked through the real window's accessibility
tree on 2026-11-13: input and saved values were both `LeseLang ✓`, Save showed
`ready` with one callback, and Other remained `draft` with zero callbacks. That
run reported nine Effects and 508 Fuel remaining. This desktop inspection
complements the native state assertions and failure tests.

On 2026-11-13, the updated main-thread `--self-test` passed all 16 groups,
including the typed-input/native-helper group. That group executed the same
text-task source twice, first with `typed native helper ✓` and then with
`second task input`. Each run returned `true` after four Effects, and fresh
observations plus direct native assertions confirmed the actual text field's
value. The group's invalid-input and forged-native-type rejection checks also
passed; the existing 32 stale-state, authority and dependency cases remained
part of the successful run.

CI checks the isolated host's formatting, builds and lints its native code, and
runs its non-GUI package tests on macOS. It does not claim desktop execution
from a headless runner. The independent Consumer's repository-boundary test
also inspects this workspace's
Cargo metadata, while continuing to require exactly five language packages in
the main workspace.

This adapter controls objects belonging to its own process. It does not provide
system-wide accessibility automation, inspect another application's UI, or
establish Windows/Linux GUI compatibility. It also does not define a wire
protocol or stable FFI ABI. The Objective-C boundary is private to this host.
The [local protocol](host-protocol.md) describes the reusable contracts for
additional adapters.
