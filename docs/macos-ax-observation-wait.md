# Explicit Bounded Observation Waiting

A successful GUI action acknowledgement can arrive before a later observation
shows the requested state. The macOS Accessibility host offers an optional
Rust `ObservationWait` for this boundary. It repeats reads for one already
dispatched `gui.observe` request under an explicit host policy, then gives the
caller a receipt to accept into that same VM session.

This host-selected policy adds no LeseLang syntax or operation,
and leaves the existing profile catalogs and ordinary `gui.observe` behavior
unchanged. The default `widgets::run` driver still accepts its immediate
observation. A host that wants waiting chooses the observation, predicate and
budgets explicitly; it does not infer a retry policy from a failed source-level
comparison.

When the source should own the expected value and budgets, use the separate
[source-declared widget wait catalog](macos-ax-source-waits.md). Its opt-in
`gui.wait_value` operation reuses this engine. The Rust policy interface
described here remains available without adding an operation to the original
catalog.

The earlier [Finder record](macos-ax-widgets.md#recorded-desktop-check) illustrates
the need: choosing text size 16 was acknowledged, the immediate observation
still returned 13, and source stopped with `false`. A later explicit read
confirmed 16. That historical result remains valid; the new opt-in host policy
does not change what the original driver observed.

## Driving A Wait

Keep the normal `poll → authorize → dispatch → accept` sequence. A wait occupies
the interval between dispatch and acceptance of one observation:

1. Poll and authorize the request, then dispatch it normally with the VM's
   original Fuel. Mutations still have their separate explicit delivery pump.
2. At a host-selected `gui.observe` receipt, begin a wait with a typed
   `FieldEquals` predicate, an attempt limit, a polling interval and a deadline.
   The dispatched snapshot supplies the first observation attempt.
3. A `Waiting` outcome gives the caller the next due time. Return control to
   the host; its event loop or reference driver decides when to call `step`.
4. Each due step performs another bounded observation under the same pending
   execution and target identity. It never invokes the preceding mutation.
5. A `Ready` outcome returns the matching observation receipt. The caller
   explicitly accepts it, then polls again so the original source can evaluate
   the observed fields and continue.

The wait implementation does not sleep, spawn a task, install a timer or pump
GUI actions. A simple reference executable can explicitly sleep until the next
due time; an interactive Rust host can return to its own event loop instead.
Neither approach changes the source program or creates a second VM session.

## Rust API And Budgets

The host's `observation_wait` module exposes the following types:

| Type | Purpose |
| --- | --- |
| `FieldEquals { field, expected }` | Selects a typed `host::Field` and its exact expected `ScalarValue`. |
| `WaitPolicy { max_attempts, interval, timeout }` | Bounds the total number of predicate observations and the scheduling interval and duration. |
| `WaitStatus { attempts, next_at, deadline }` | Reports progress and due times in the caller's clock domain. |
| `ObservationWait<S>` | Owns one pending observation wait; `status()` exposes its scheduling information. |
| `WaitStep<S>` | Either `Waiting(ObservationWait<S>)` or `Ready(ProfileReceipt<S>)`. |
| `WaitError` | Distinguishes policy, predicate, session, clock, expiration and host failures. |

The default policy allows 16 attempts with a 50 ms interval and a 2 s timeout.
Explicit policies allow 1–64 attempts, intervals from 1 ms to 1 s, and timeouts
from 1 ms to 30 s; the interval must not exceed the timeout. In this interface,
the embedding host supplies these budgets outside the source program.
An attempt means one snapshot predicate evaluation, not one AX API call.
Calling `step` before `next_at` still checks live authority and spends Fuel;
those checks may read native metadata without adding a predicate attempt.
Schedule the next step from `next_at` instead of continuously calling it.

The clock is a caller-supplied `FnMut() -> Duration`. It must advance
monotonically within the admitted clock range. The initial call at `begin`
establishes the deadline; the already completed original dispatch is not part
of that wait duration. That dispatched snapshot still counts as attempt one.
Clock regression, out-of-range values and arithmetic overflow refuse the wait.

`Ready` does not accept the receipt. Pass it through the existing
`driver::accept` boundary before polling source again. Dropping a waiting handle
does not cancel its VM: the embedding host must explicitly cancel the session
when abandoning the wait. A foreign VM, host or execution is rejected without
closing an unrelated session; a failed wait in its own session ends the
pending work while retaining any earlier side effects.
Each observation request can start only one wait. Starting again with another
copy of its receipt cannot reset the attempt or deadline budget.

For a widget observation that has been dispatched but not accepted, the
embedding host can begin an exact value wait like this:

```rust
use leselang_macos_ax_host::{
    driver,
    host::{Field, Host, ProfileReceipt},
    observation_wait::{self, FieldEquals, WaitError, WaitPolicy, WaitStep},
    widgets::WidgetSnapshot,
};
use leselang_vm::native::core::{OptionalStringValue, ScalarValue};
use std::time::Duration;

fn begin_value_wait(
    vm: &mut driver::Vm<'_, '_, WidgetSnapshot>,
    host: &Host<'_, '_, WidgetSnapshot>,
    receipt: ProfileReceipt<WidgetSnapshot>,
    clock: &mut impl FnMut() -> Duration,
) -> Result<WaitStep<WidgetSnapshot>, WaitError> {
    observation_wait::begin(
        vm,
        host,
        receipt,
        FieldEquals {
            field: Field::Value,
            expected: ScalarValue::OptionalString(OptionalStringValue(Some("16".into()))),
        },
        WaitPolicy::default(),
        clock,
    )
}
```

When the result is `WaitStep::Waiting(wait)`, retain the handle, inspect
`wait.status().next_at`, and explicitly call
`observation_wait::step(vm, host, wait, clock)` when due. Continue handling the
returned `WaitStep` in the same way. The complete
[`finder_wait.rs`](../hosts/macos-ax/examples/finder_wait.rs) reference shows the
polling, mutation pumping, wait scheduling and acceptance boundaries together.

## Identity, Authority And Time

The predicate compares one typed snapshot field with an exact expected value.
For example, a widget's optional value `Some("16")` is distinct from an absent
value or `Some("")`. Waiting for a value does not search again by label or choose
another object that happens to have that value.

The target reference, execution, native object, application process and window
remain fixed. Snapshot revisions may advance while the application makes
progress; a wait must not reject the intended value change merely because it
has a newer revision. Replacement, retirement or loss of authority still
refuses the wait. Permission checks include any explicit
[auxiliary-process grant](macos-ax-auxiliary-processes.md), and normal reply
acceptance checks run again before source resumes.

Attempt counts and the deadline bound repeated observations. The caller supplies
the clock, which is checked around native work; a deadline is cooperative and
does not preempt an AX call already in progress. A slow native callback may
therefore return after the requested time limit. The host must treat elapsed
time, attempt exhaustion and native errors according to their distinct outcomes,
without converting a failed authority or identity check into an ordinary
nonmatching value.
An observation that completes at or after the deadline does not return `Ready`,
even if its value matches. A final permitted attempt can still succeed if it
matches before that deadline; otherwise the attempt ceiling ends the wait.

Additional observations and predicate evaluation use the original VM Fuel.
Waiting does not refill that budget or obtain a new execution lease. A timeout,
cancellation or exhausted budget must stop dependent source actions. Earlier
delivered effects remain delivered, and a late successful application update
does not retroactively resume an ended wait.

A matching result is a historical observation. Another process may change
after the read and before reply acceptance or a subsequent action. Waiting
does not provide atomic compare-and-set, prove that rendering completed or
detect every unseen A→B→A change. A later mutation still needs an observed
revision and the usual immediate native preflight.

## Finder Reference Scope

The reference host uses the existing
[`widgets_finder_options.lese`](../hosts/macos-ax/examples/widgets_finder_options.lese)
source. It explicitly chooses the first owner observation after the
`gui.choose_menu_item` operation and waits for its optional `value` field to
equal `Some("16")`. The source's own final comparison then verifies 16 after
the matching receipt is accepted. Earlier observations, menu opening,
cancellation and selection keep their existing immediate behavior.

Prepare a dedicated folder such as `target/observation-wait-fixture` with three
empty subfolders. Open it in Finder's list view and show its `ViewOptions`
window. Set text size 13 for this test folder and leave **Use as Defaults**
unused. The [widgets guide](macos-ax-widgets.md#finder-reference-workflow)
explains exact window discovery and the expected popup metadata. The waiting
reference needs no second window, row selection or scrolling.
After preparing the exact window, run the reference from the repository root:

```sh
cargo run --manifest-path hosts/macos-ax/Cargo.toml --example finder_wait --locked --offline --profile lean -- 'observation-wait-fixture'
```

The argument is the actual View Options window title. The example combines it
with Finder's exact bundle path and the `ViewOptions` identifier; it does not
select a focused window. It uses a monotonic `Instant` clock, 10,000 initial
Fuel, and the default wait policy. Only the executable's explicit scheduling
branch sleeps. Ordinary language Effects, including the menu choice, are
still dispatched once each.

Independently inspect the final popup value after a run. Record the number of
observation attempts: a first-attempt success exercises the fast path, while a
delayed desktop result requires evidence of more than one attempt. Deterministic
backend and clock tests cover delayed progress even when the real application
updates immediately.

To exercise expiration without repeating a mutation, use a separate read-only
wait whose expected value does not occur. It must stop at its configured bound
without executing a dependent action. Never rerun the menu choice merely to
compensate for an earlier observation that had not yet updated.

The reference provides that read-only check:

```sh
cargo run --manifest-path hosts/macos-ax/Cargo.toml --example finder_wait --locked --offline --profile lean -- 'observation-wait-fixture' --timeout-check
```

It selects and observes the text-size popup, then waits for a deliberately
nonmatching value with a 250 ms timeout, 50 ms interval and 64-attempt ceiling.
An expiration error and nonzero exit are the expected result. This mode performs
no menu selection or other mutation.

## Deterministic Validation

The independent
[`observation_wait_api`](../hosts/macos-ax/tests/observation_wait_api.rs) tests
use a portable backend and an explicitly advanced clock. They require no
desktop, wall-clock sleeps or background worker:

```sh
cargo test --manifest-path hosts/macos-ax/Cargo.toml --test observation_wait_api --locked --offline --profile lean
```

Coverage includes immediate and delayed matches, historical observations,
separate time and attempt limits, a matching read that returns too late,
permission and auxiliary-grant revocation, cancellation, replacement, native
read errors, original Fuel exhaustion and foreign sessions. A delayed write
fixture verifies that waiting never repeats the delivered mutation and that
expiration preserves its partial effects. Keep the ordinary widgets source
tests as well: opting into waiting must not change their immediate-read
behavior or add operations to the original catalogs.

## Recorded Desktop Check

On 2026-11-13, the reference ran against Finder 27.0 on macOS 27.0.1 with
existing Accessibility access. Its dedicated
`target/observation-wait-fixture` contained three empty subfolders. Exact
selection used the title `observation-wait-fixture` and identifier
`ViewOptions`; initial widget discovery admitted 30 widgets and reported text
size 13.

The read-only `--timeout-check` ran first. Its predicate remained unmatched,
reported attempts one and two, and ended with `TimedOut` and exit status 1.
It issued no mutation. The normal reference then ran the unchanged
`widgets_finder_options.lese` source in one session:

| Check | Observed result |
| --- | --- |
| Language Effects | 11 |
| Delivered `gui.choose_menu_item` calls | Exactly one |
| First post-choice observation | `Some("13")` |
| Explicit wait | Attempt one returned `Waiting`; the next step observed `Some("16")` at revision 14 |
| Acceptance and source result | The matching receipt was explicitly accepted, then source returned `true` |
| Original Fuel remaining | 9,086 of the initial 10,000 |

This run exercised actual delayed publication: it needed two predicate
observations, without repeating the chosen action or starting another VM.
Afterward, a separate CLI connection independently discovered text size 16
and confirmed that Size and Kind remained checked. Desktop screenshot
inspection showed the dedicated folder's larger text, but did not expose the
View Options field; the exact value 16 was verified by that separate AX
connection. The check does not claim rendering synchronization or a hard
wall-clock bound on AX calls.

At this milestone, the Accessibility package passed 161 Rust tests, including
18 external wait-contract tests and five internal wait tests; all had zero
failures or ignored tests. The 37 native boundary groups, language workspace's
2,303 tests and independent Consumer's 735 tests also passed. Formatting and
strict all-targets Clippy passed for the Accessibility package. The Rust
embedding snippet above compiled to metadata without invoking native code.
The unchanged AppKit host was not rerun for this waiting milestone.
