# A Scoped macOS Accessibility Host

The independent [`hosts/macos-ax`](../hosts/macos-ax/Cargo.toml) workspace adapts
LeseLang to a text document in another process: Apple's TextEdit application.
The host finds one explicitly selected document, observes its editable text
area, queues a text replacement and reads the actual value again to verify the
result. The compiler, native object protocol, Fuel and VM remain the existing
Rust implementations.

This adapter uses macOS Accessibility (`AXUIElement`) through a private native
bridge. It has a narrower operation catalog than the application-owned
[AppKit form host](appkit-host.md). Framework dependencies and target-specific
link flags stay outside the five language crates and the independent Consumer.

## Prepare And Run

Requirements:

- macOS 14 or later, TextEdit and a logged-in desktop session.
- Rust 1.98.0 and Xcode or Xcode Command Line Tools with a macOS 14+ SDK.
- Accessibility permission for the process running the host.
- An existing plain-text document already open in TextEdit, selected by its
  exact local file path.

Build from the repository root:

```sh
cargo build --manifest-path hosts/macos-ax/Cargo.toml --locked --offline --profile lean
target/lean/leselang-macos-ax-host --check-permission
```

`--check-permission` reports whether the current process has Accessibility
access. It does not prompt, change system settings or start TextEdit. If access
is unavailable, the command fails before GUI work. Permission granted to another
tool does not establish that this executable has access. Any required access
must be arranged explicitly in macOS before running the adapter again.

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

Source files must contain valid UTF-8 and are limited to 16 KiB (16,384 bytes).
The runner starts with 1,000 Fuel. Custom source supplies its own checks and
final scalar result; the runner does not impose the demo's initial or final
text. A native object or observation cannot escape as the final result: project
scalar fields before returning. Rejecting a native final result does not undo
text already sent to TextEdit. Printed strings escape control characters and
are intended for human inspection, not as a wire format.

## Operations And Observations

| Operation | Inputs | Result |
| --- | --- | --- |
| `gui.find` | `name: "document"` | An opaque object reference for the scoped text area, bound to this execution |
| `gui.observe` | `target`: that reference | A historical snapshot of the text area |
| `gui.set_text` | `target`, `expected_revision`, `text` | An acknowledgement that the replacement was queued |

The literal name `document` selects the host's explicitly configured document.
It does not select an arbitrary application, window or file. The CLI path is
part of host configuration, while the returned native object reference remains
necessary for observation and mutation.

Snapshots expose scalar `document`, `role`, `value`, `editable` and `revision`
fields. The document is the matched file URL; the role must be `AXTextArea`.
These values describe an observation. They do not grant authority, retain the
remote process or turn an old snapshot into current state. `gui.set_text`
replaces the complete text value through Accessibility; it does not synthesize
keyboard events or depend on which application has focus.

The [host catalog](../hosts/macos-ax/src/host.rs) and
[driver](../hosts/macos-ax/src/driver.rs) keep the handoff explicit:

1. `poll` yields a typed Effect request using the original session Fuel.
2. The host authorizes the current execution and checks live Accessibility access.
3. `dispatch` reads state or queues a text replacement without delivering it.
4. For a queued replacement, the driver explicitly calls `pump`. The host
   rechecks permission, object identity, observed state and editability before
   entering the native setter.
5. The driver accepts the reply and polls again. Source requests a new
   observation to verify the resulting value.

There is no retry loop, background scheduler or implicit wait for the desired
value. A submission acknowledgement alone does not prove that the document
changed, and a successful setter does not replace the source's final observation.

## Process, Document And Object Scope

The native connection is limited to the system application at
`/System/Applications/TextEdit.app`, whose bundle identifier must be
`com.apple.TextEdit`. A bounded `libproc` scan examines at most 4,096 processes
owned by the current user and requires exactly one matching executable path.
It does not select by display name or foreground application. The connection
retains that executable path, PID and process start time, then checks them again
before accessing the target. A PID reused by a later process must not inherit
the old connection.

Window selection matches the canonical local file URL against `AXDocument`.
The bridge searches only within that matched window for the editable text area.
Window and child lists are limited to 64 entries, and the scoped window's tree
walk is limited to 128 nodes and depth 16. Excess nodes, excessive depth,
ambiguous matches and unexpected native value types fail rather than selecting
the first convenient control. Empty child lists are checked before a bounded
array read, because AX can reject index zero on an empty collection. The host
also enforces the language's 4,096-byte
per-string bound before admitting snapshot text or a replacement value.

Retained window and text-area Accessibility references are checked against the
current scoped objects with `CFEqual`, together with process and document
identity. A closed window, replaced element, different document or restarted
process invalidates the connection. The host does not silently rebind an old
LeseLang reference to a newly discovered object with similar visible text.

## Freshness Across Processes

The existing local object protocol is unchanged. Its revision still fences
observed state changes and consumes each admitted action attempt. The native
transport determines how much external history that revision can represent.

The application-owned AppKit host records every relevant mutation, including
changes from A to B and back to A. TextEdit is a separate process, and this AX
adapter observes snapshots. A change that happens and is reversed entirely
between observations can go unseen. The bridge compares the latest observed
state immediately before entering the setter, but the other process can still
change between that read and the write. This is not an atomic remote
compare-and-set operation or a guarantee against all external ABA changes.

The same limitation applies to a snapshot's several Accessibility attributes:
they are bounded reads of remote state, not a transaction with TextEdit.
Permission, process identity and element checks narrow the target and reject
detected changes; they cannot lock the external application's state. Source
must observe after the action and handle a mismatched result explicitly.

## Failures And Verification

Unavailable Accessibility access, a missing scoped document, stale identity,
uneditable text, traversal limits and exhausted Fuel refuse work before the
setter is entered. Checks occur again when queued work is pumped. Cancellation
before delivery prevents the queued edit; cancellation after delivery cannot
undo an edit already performed by TextEdit. Late replies remain subject to the
same execution and correlation checks as other native hosts.

Once the Accessibility setter has been entered, an error or timeout is an
uncertain outcome. The target application may have applied some or all of the
edit before the failure became visible. The host preserves that distinction and
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

Those four groups check UTF-8 limits, canonical local document URLs, invalid
handles and output publication, and AX error/empty-array handling. They make
no remote Accessibility calls and do not require or grant permission.

Deterministic backend tests establish the host's local queues, reply validation,
Fuel, cancellation and error classification. They do not establish that an
external application was read or changed. Real desktop acceptance requires the
prepared file to be open in TextEdit, a successful permission check, a run of
the source, and a separate check of the application's resulting text. A denied
permission check or a successful compile is not a successful cross-process GUI
run.

CI runs this package's deterministic tests, formatting and strict Clippy on
Linux and macOS, plus the native boundary checks on macOS. The native AX
implementation is compiled on macOS; portable backend tests on Linux do not
execute the Apple frameworks. No CI test claims an authorized TextEdit desktop
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

The host package also passed 15 deterministic backend tests, five command-line
tests and one compile-fail ownership test on macOS. These results complement
the real run; the deterministic tests cover failure paths without manipulating
the user's other applications or changing OS permissions.

The adapter currently targets one plain-text TextEdit document on macOS. Other
applications, rich-text operations, application launching, saving, generalized
GUI discovery, Windows/Linux backends and a stable remote wire protocol remain
separate work. Local object references are never exposed as reusable remote
handles.

Apple's [Accessibility API reference](https://developer.apple.com/documentation/applicationservices/axuielement)
describes the underlying calls. Its
[messaging timeout documentation](https://developer.apple.com/documentation/applicationservices/1459345-axuielementsetmessagingtimeout)
also distinguishes a per-element timeout from the current process's default.
