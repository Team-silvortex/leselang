# Source-Declared Widget State Waiting

The optional state-wait catalog lets LeseLang source wait for a checkbox state,
an item's selection state or a normalized scrollbar position. It includes the
existing `gui.wait_value` operation and adds three typed operations. The Rust
host still supplies the clock and explicitly advances each pending wait.

This catalog is a separate opt-in. Existing widgets and value-wait entry points
keep their behavior:

| CLI selection | Rust entry points | Operations |
| --- | --- | --- |
| `--widgets` | `widgets::with_host`, `widgets::run` | Original 13-operation widget catalog. |
| `--widgets --wait-values` | `widgets::with_wait_host`, `widgets::run_with_waits` | 14 operations, including `gui.wait_value`. |
| `--widgets --wait-states` | `widgets::with_state_wait_host`, `widgets::run_with_state_waits` | 17 operations, including value, checkbox, selection and position waits. |

The two wait flags are mutually exclusive and require `--widgets --run SOURCE`
with an explicitly selected application and window. Neither is a discovery
action. The original catalogs reject the new state operations before source
Effects run; enabling value waiting alone does not enable state waiting.

## Typed Conditions And Availability

Each new operation has five required arguments and returns a snapshot:

```text
gui.wait_checked(target, expected: boolean, max_attempts, interval_ms, timeout_ms)
gui.wait_selected(target, expected: boolean, max_attempts, interval_ms, timeout_ms)
gui.wait_position(target, expected: integer, max_attempts, interval_ms, timeout_ms)
```

Calls use the existing named-argument syntax. `target` must be a live widget
`ObjectRef` in the current window and execution. All three budget arguments
are integers. `max_attempts` is 1–64, `interval_ms` is 1–1,000, and `timeout_ms`
is 1–30,000; the interval must not exceed the timeout. Position expectations
must be within 0–1,000. Booleans, integers and strings are not implicitly
converted into one another.

The condition checks both the native state and whether that state is available:

| Operation | Matching state | Unavailable or other states |
| --- | --- | --- |
| `gui.wait_checked` | `true` matches `checked`; `false` matches `unchecked` on an `AXCheckBox`. | An absent check state or wrong role is an error. `mixed` is a valid nonmatching state for either boolean and can continue waiting. |
| `gui.wait_selected` | An available selected state equals the expected boolean. | Missing selection state is an error; the snapshot's fallback scalar `selected: false` is not evidence of an unselected item. |
| `gui.wait_position` | An available normalized scrollbar position equals the expected integer. | Missing position state is an error; the snapshot's fallback scalar `position: 0` is not evidence that the scrollbar is at its start. |

Pending steps recheck current availability, including calls made before the
next predicate attempt is due. Losing the required state does not become an
ordinary mismatch that can silently match `false` or `0`. Permission and
identity failures likewise stop the wait. An enabled action capability is a
separate question: observing a state does not itself authorize a mutation.
Selection waiting also pins the item's collection owner when the original
Effect request is created. Moving the same item into a different collection
between polling and beginning the wait, or during a pending wait, fails
instead of satisfying the condition in a new context.

For example, source can request an unchecked state after an independently
authorized checkbox action, then verify the returned snapshot:

```text
fn main() = bind(
    target: gui.select(role: "AXCheckBox", identifier: "", label: "Size"),
    body: bind(
        before: gui.observe(target: target),
        body: bind(
            submitted: gui.set_checked(
                target: target,
                expected_revision: field(value: before, name: "revision"),
                checked: false,
            ),
            body: bind(
                after: gui.wait_checked(
                    target: target,
                    expected: false,
                    max_attempts: 16,
                    interval_ms: 50,
                    timeout_ms: 2000,
                ),
                body: eq(left: field(value: after, name: "check_state"), right: "unchecked"),
            ),
        ),
    ),
)
```

This example only applies to the prepared test window. A wait never presses the
checkbox itself or repeats the prior action. Waiting for one selected item
also does not establish that it is the collection's only selected item; source
must separately check the owner's `selected_keys` when exclusivity matters.

## Explicit Rust Driving

Use `widgets::with_state_wait_host` when owning the VM loop, or
`widgets::run_with_state_waits(backend, source, clock, wait_until)` for the
convenience driver. The callbacks have the same contract as
[source value waiting](macos-ax-source-waits.md#rust-host-responsibilities):
the clock returns monotonic elapsed `Duration` values, and `wait_until`
receives an absolute due time in that clock's origin.

A standalone Rust host can explicitly choose a sleeping callback:

```rust
use leselang_macos_ax_host::{
    driver::ProfileRunSummary,
    widgets::{self, WidgetSnapshot, WidgetsBackend},
};
use std::time::{Duration, Instant};

fn run_state_wait_source(
    backend: &dyn WidgetsBackend,
    source: &str,
) -> Result<ProfileRunSummary<WidgetSnapshot>, String> {
    let origin = Instant::now();
    let mut clock = || origin.elapsed();
    let mut wait_until = |due: Duration| {
        std::thread::sleep(due.saturating_sub(origin.elapsed()));
    };
    widgets::run_with_state_waits(backend, source, &mut clock, &mut wait_until)
}
```

The generic driver routes a source wait work item to
`observation_wait::begin_source`, advances a `Waiting` handle through explicit
`step` calls and explicitly accepts its `Ready` receipt. Ordinary dispatch
cannot complete these operations, and an unrelated or reconstructed snapshot
cannot substitute for the admitted ready result. Source supplies the typed
condition; the driver does not special-case Finder labels or prior actions.

The library installs no timer, sleep, worker or asynchronous runtime. The CLI
explicitly supplies a synchronous sleeping callback. Interactive hosts can
retain the waiting handle and return to their own event loop. The original VM
Fuel pays for every predicate observation and comparison; one wait remains
one language Effect regardless of its number of attempts.

The deadline includes the initial native observation and is checked around
native work. It is cooperative and cannot interrupt an AX call or a host
callback already in progress. A matching sample that completes at or after
the deadline fails. Timeout, cancellation, budget exhaustion and authority
loss stop dependent source work without undoing an earlier delivered action.

All returned snapshots remain historical. Once a matching sample has completed,
a later change to the live value or its availability does not rewrite that
snapshot. Reply acceptance still rechecks the execution, object identity,
current permissions and exact ready result. A later mutation needs its own
observed revision and native preflight. State waiting does not provide atomic
remote compare-and-set or rendering synchronization.

## Finder Reference Procedure

Use a new dedicated `target/state-wait-fixture`; leave earlier fixtures and
their recorded results intact. The reference has two sessions with separately
selected windows. Finder can give both windows the same title, so combine
that title with the identifiers `ViewOptions` and `FinderWindow`.

Start with three empty subfolders named `Widget folder 01` through
`Widget folder 03`. Open the directory in Finder's list view, hide the sidebar
and sort Name ascending. In its View Options, choose text size 16 and large
icons, enable Size and Kind, and disable the other optional columns, including
Date Modified. These settings belong to this test folder; leave **Use as
Defaults** unused.

Inspect View Options, then run the
[checkbox source](../hosts/macos-ax/examples/widgets_finder_wait_columns.lese):

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'state-wait-fixture' --window-id ViewOptions --widgets --discover
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'state-wait-fixture' --window-id ViewOptions --widgets --wait-states --run hosts/macos-ax/examples/widgets_finder_wait_columns.lese
```

The source first verifies text size 16. It requests Size unchecked, calls
`gui.wait_checked(expected: false)`, verifies `unchecked`, then does the same
for Kind. A failed wait or postcondition stops the next operation. No menu
action is part of this phase.

After the options phase, expand the fixture to 12 empty subfolders, ending at
`Widget folder 12`, and close View Options through the ordinary UI. Leave all
rows unselected and the scrollbar at the top. Make the list window short
enough that the large rows require vertical scrolling and wide enough to
avoid a horizontal scrollbar. Hide the status bar as well, then use discovery
to confirm that `statusBarText` is absent from the AX tree. A hidden-looking
bar or a menu labelled **Show Status Bar** is not sufficient evidence: during
the recorded run, showing and then hiding the bar through the ordinary UI
removed a retained AX status node. Its changing available-disk-space text
otherwise participates in the existing whole-window revision checks and can
invalidate an observed revision before a later mutation. Do not ignore that
state or bypass the revision check to make a test pass.

With the sidebar and status bar hidden and optional columns
removed, discovery must show the supported inventory within its existing
64-widget limit. The wait catalog does not increase discovery bounds.

Inspect the separately selected folder window, then run the
[selection and scrolling source](../hosts/macos-ax/examples/widgets_finder_wait_selection.lese):

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'state-wait-fixture' --window-id FinderWindow --widgets --discover
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'state-wait-fixture' --window-id FinderWindow --widgets --wait-states --run hosts/macos-ax/examples/widgets_finder_wait_selection.lese
```

The source finds the exact filename value `Widget folder 01`, verifies its
`AXCell` and `AXRow` ancestors and checks row membership in `ListView`. It
selects the row once and calls `gui.wait_selected(expected: true)` on that
actual row reference. A separate owner observation must then show exactly
that one selected key before scrolling can proceed.

For scrolling, source requires one enabled vertical scrollbar with an
available position of 0, submits position 1,000 once, then calls
`gui.wait_position(expected: 1000)` and verifies the returned position and
availability. Selection does not open a folder. Independently inspect the
final selection, scroll position and checkbox state afterward. Record whether
each wait matched immediately or needed another predicate observation; a
successful immediate match is valid but does not demonstrate a delayed update.

## Deterministic Validation

The public [state-wait tests](../hosts/macos-ax/tests/state_wait_api.rs) use a
portable backend and a caller-controlled clock:

```sh
cargo test --manifest-path hosts/macos-ax/Cargo.toml --test state_wait_api --locked --offline --profile lean
```

They exercise rejection by the original catalogs, strict argument types,
mixed checkbox progress, absent state, loss of availability before a step is
due, collection membership changes and exact normalized positions. Other
cases cover historical ready snapshots, identity replacement, reply substitution
and transfer between wait kinds, cancellation, permission and auxiliary-grant
revocation, shared Fuel, deadline and attempt limits, and preserving an earlier
native delivery without repeating it. Existing value-wait and host-policy wait
tests retain their separate compatibility contracts.

## Recorded Desktop Verification

On 2026-11-13, the dedicated Finder fixture completed both new source programs
on macOS 27.0.1 with the explicit `--wait-states` catalog:

| Session | Observed result | Effects | Fuel remaining |
| --- | --- | --- | --- |
| View Options | Size and Kind each changed to unchecked once, and each `gui.wait_checked` returned the matching snapshot. | 10 | 9,187 |
| Folder window | `Widget folder 01` selected once, the owner independently confirmed that sole selection, then one scroll reached normalized position 1,000. | 15 | 8,567 |

Both programs returned `true`. All four typed waits matched their first
predicate observation; this desktop run does not claim to demonstrate a
delayed state update. Delayed matching and failure paths are covered by the
deterministic tests. The earlier value-wait menu example was also used to
prepare text size 16 through the existing `--wait-values` entry point; that
was a separate 14-operation-catalog session.

A new native connection independently confirmed text size 16 and the
unchecked Date Modified, Size and Kind options after the options session.
After the successful folder session, another connection discovered 56 widgets,
exactly one selected row, the `Widget folder 01` text field linked through its
cell to that row, and the unique vertical scrollbar at 1,000. The independent
UI accessibility observation agreed on the selected row and scrollbar value
1; the screenshot showed the final folder at the bottom of the list.

Two earlier folder sessions were refused by the original `StaleRevision`
checks. The first had already selected and verified the row, then refused
scroll before submission; the selection remained. After explicitly clearing
that selection, a new session refused `select_item` before any mutation.
During these runs, discovery and UI observations exposed changing available
disk space in `statusBarText`; the native metadata includes that value in the
whole-scope revision. After hiding the bar and confirming the node had left
the AX tree, the successful run started as another new session with no
selection and position 0. No failed work item was replayed and no revision
guard was weakened. The observations identify a concrete source of revision
changes, rather than proving that every earlier change had only that cause.

The original `--wait-values` catalog also rejected the new selection source
at compilation before polling any Effect. Final regression results were
2,303 language tests, 735 independent Consumer tests, 208 AX adapter tests
(including 18 public state-wait tests), and 37 native bridge boundary groups,
all passing. Strict all-target Clippy, formatting and diff checks passed.
