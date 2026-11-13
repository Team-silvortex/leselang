# Source-Declared Widget Value Waiting

The optional widget wait catalog lets LeseLang source declare the value it
expects and how much repeated observation it permits. Its `gui.wait_value`
operation returns a matching widget snapshot under the same execution and
target identity. The Rust host still supplies the clock and explicitly decides
when to advance the wait.

This is a new host operation using existing language syntax. The original
widgets catalog keeps its 13 operations, `widgets::with_host` and
`widgets::run` keep their existing behavior, and `gui.observe` still returns
an immediate observation. Enabling the separate wait catalog adds one operation,
for a total of 14. TextEdit, Calculator and the six-operation controls catalog
do not acquire it.

For checkbox, selection and scrollbar-position conditions, the independent
[state-wait catalog](macos-ax-state-waits.md) provides a 17-operation opt-in
through `--wait-states`. It includes `gui.wait_value`, while the 14-operation
`--wait-values` interface described here remains unchanged.

The lower-level [observation wait](macos-ax-observation-wait.md) remains useful
when a Rust embedder owns the postcondition policy. That API lets the host choose
an observation and predicate. With `gui.wait_value`, the source declares the
target, expected value and budgets directly; the driver does not infer them
from a preceding menu operation, a Finder label or a failed comparison.

## Operation Contract

```text
gui.wait_value(
    target: ObjectRef,
    expected: optional_string,
    max_attempts: integer,
    interval_ms: integer,
    timeout_ms: integer,
) -> Snapshot
```

The target must already be a live object reference in the selected widget
window and current execution. It is not a discovery key or serialized native
identity. Waiting reads that same object's optional `value` field; it does
not repeat a selector or switch to another matching control.

All five arguments are required. The expected value uses the existing
`optional_string` constructor:

| Source value | Meaning |
| --- | --- |
| `optional_string(value: "16")` | Present text exactly equal to `16`. |
| `optional_string(value: "")` | Present empty text. |
| `optional_string(value: none)` | An absent value on the still-live widget. |

A bare string is not implicitly converted to an optional string. An absent
value does not mean that a missing or replaced widget satisfies the condition;
identity and authority failures still refuse the operation.

`max_attempts` must be 1–64, `interval_ms` must be 1–1,000, and `timeout_ms`
must be 1–30,000. The interval must not exceed the timeout. These bounds apply
to each wait. All observations and comparisons share the original VM Fuel;
several source waits do not create several fresh Fuel budgets.
One `gui.wait_value` call is one language Effect. Its repeated observations are
metered host work while that request remains pending, not new source calls or
new sessions.

A source can explicitly wait for the popup value and verify the returned
snapshot without adding a language-level loop:

```text
fn main() = bind(
    owner: gui.select(role: "AXPopUpButton", identifier: "Text size:", label: ""),
    body: bind(
        observed: gui.wait_value(
            target: owner,
            expected: optional_string(value: "16"),
            max_attempts: 16,
            interval_ms: 50,
            timeout_ms: 2000,
        ),
        body: eq(
            left: value_or(left: field(value: observed, name: "value"), right: ""),
            right: "16",
        ),
    ),
)
```

This small source only observes. The Finder reference below separately submits
a menu choice before waiting for its postcondition. Timeout and other failures
stop the pending execution; they are not a `false` snapshot or an instruction
to retry the preceding action.

## Explicit CLI Opt-In

Use `--wait-values` only with `--widgets --run SOURCE` and the existing exact
application/window selection:

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'source-wait-fixture' --window-id ViewOptions --widgets --wait-values --run hosts/macos-ax/examples/widgets_finder_wait_value.lese
```

Without `--wait-values`, a source containing `gui.wait_value` is rejected by
the original catalog before its Effects execute, including any menu actions
written earlier in that source. The flag is not valid with `--discover`,
`--controls`, a legacy profile or the standalone application/window discovery
actions. Native widget discovery itself is unchanged; use ordinary
`--widgets --discover` to inspect the selected window before a source run.

Existing [auxiliary-process grants](macos-ax-auxiliary-processes.md) can be
combined with the wait-enabled widget run. This adds no auxiliary authority or
new native traversal. The selected process, window and any approved helper
instances remain subject to the usual live checks.

## Rust Host Responsibilities

`widgets::with_wait_host` exposes the 14-operation catalog for a caller-owned
driver. `widgets::run_with_waits` supplies a convenience driver with explicit
clock and `wait_until` callbacks. The clock reports monotonic elapsed
`Duration` values; the scheduling callback uses that same time origin.
An embedder may integrate the lower-level wait steps into its own event loop.
The CLI's scheduling callback sleeps explicitly until a requested due time.

A standalone Rust host can choose that scheduling policy directly:

```rust
use leselang_macos_ax_host::{
    driver::ProfileRunSummary,
    widgets::{self, WidgetSnapshot, WidgetsBackend},
};
use std::time::{Duration, Instant};

fn run_wait_source(
    backend: &dyn WidgetsBackend,
    source: &str,
) -> Result<ProfileRunSummary<WidgetSnapshot>, String> {
    let origin = Instant::now();
    let mut clock = || origin.elapsed();
    let mut wait_until = |due: Duration| {
        std::thread::sleep(due.saturating_sub(origin.elapsed()));
    };
    widgets::run_with_waits(backend, source, &mut clock, &mut wait_until)
}
```

The callback is the embedding application's synchronous code. Fuel does not
preempt a blocked callback; the host remains responsible for its scheduling
and callback work limits. An interactive application can use the caller-owned
driver below instead of sleeping on its UI thread.

The wait engine does not sleep, spawn a worker or install an asynchronous
runtime. It reuses `ObservationWait` and its original-Fuel accounting,
permission checks and explicit `begin`/`step` progression. Only a completed
wait receipt can resume the source. Its reply stays bound to the requesting
Effect and target, and ordinary reply acceptance remains explicit.

For a caller-owned driver, route a polled `gui.wait_value` work item directly
to `observation_wait::begin_source(vm, host, work, clock)`. This returns the
same `Waiting` or `Ready` states as the existing wait engine. Advance a waiting
handle with `observation_wait::step`, then explicitly accept a ready receipt.
Ordinary `driver::dispatch` refuses the new operation, and the host-policy
`begin` only accepts ordinary observation receipts. A source wait cannot be
satisfied by accepting an uncompleted or reconstructed snapshot receipt.

The source wait's deadline begins at the first clock reading in `begin_source`
and includes its initial observation, which is attempt one. This differs from
the host-policy `begin` API: that API receives an already-dispatched
`gui.observe` receipt and starts its deadline after that original read.

The returned snapshot is historical. Its target can have a newer revision
because the value changed while waiting; replacement and retirement remain
errors. A later mutation still needs its observed revision and native preflight.
Successful waiting does not promise that the value remains unchanged after the
read, synchronize rendering or provide atomic remote compare-and-set.

Attempt limits count predicate observations, not individual AX API calls.
Authority checks can themselves read native metadata. Time limits are
cooperative: an in-progress AX call cannot be preempted, and a matching sample
that finishes at or after the deadline does not succeed. An already delivered
action remains delivered when a later wait expires, loses permission or runs
out of Fuel. The adapter never retries that action to satisfy the wait.

## Finder Reference Procedure

Create a new dedicated `target/source-wait-fixture` with three empty subfolders.
Open it in Finder's list view, show its View Options and set text size 13 for
this folder. Leave **Use as Defaults** unused. The earlier
`observation-wait-fixture` can remain at its verified value of 16; do not reuse
it as though it still satisfied the initial condition.

Discover the current window and confirm the exact popup metadata:

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --windows
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'source-wait-fixture' --window-id ViewOptions --widgets --discover
```

Use the actual title and identifier if the application reports different
metadata. The reference expects an `AXPopUpButton` with identifier
`Text size:` and a menu item labeled `16`.

Run the separate [read-only expiration source](../hosts/macos-ax/examples/widgets_wait_value_timeout.lese)
first:

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'source-wait-fixture' --window-id ViewOptions --widgets --wait-values --run hosts/macos-ax/examples/widgets_wait_value_timeout.lese
```

It waits for a deliberately nonmatching value with 64 maximum attempts, a
50 ms interval and a 250 ms timeout, and must end without a mutation.
Then run [the menu source](../hosts/macos-ax/examples/widgets_finder_wait_value.lese)
shown under CLI opt-in. It requires initial value 13,
opens and cancels the menu, observes that 13 remains, opens again, chooses 16
once and explicitly calls `gui.wait_value` for 16. The source then checks the
returned snapshot; the host has no Finder-specific postcondition branch.

Afterward, open a new read-only connection and independently confirm value 16
and that unrelated checkbox state remains unchanged. Record how many predicate
attempts were needed. A first-attempt match exercises a valid fast path;
claim delayed desktop publication only if the run actually needed another
observation. The existing host-policy and original immediate-read Finder
records remain separate milestones.

## Deterministic Validation

The public [source wait tests](../hosts/macos-ax/tests/source_wait_api.rs) use a
portable widget backend and an explicit fake clock:

```sh
cargo test --manifest-path hosts/macos-ax/Cargo.toml --test source_wait_api --locked --offline --profile lean
```

They exercise catalog opt-in, cold rejection by the old catalog, exact argument
types, missing versus empty optional values, budget admission and initial-read
deadline accounting. They also cover explicit progression and acceptance,
attempt exhaustion, native errors, replacement, cancellation, live permission
and auxiliary-grant checks, original Fuel, and preservation of earlier effects.
Bypass tests reject ordinary dispatch, direct acceptance, substituted historical
snapshots, reuse of another request's ready result and overriding source intent
through the host-policy wait entry point. The original observation-wait tests
and immediate-read widget examples remain separate regression coverage.

## Recorded Desktop Check

On 2026-11-13, the CLI ran against Finder 27.0 on macOS 27.0.1 with existing
Accessibility access. The new `target/source-wait-fixture` contained three
empty subfolders. Exact selection used title `source-wait-fixture` and
identifier `ViewOptions`. Initial discovery admitted 30 widgets, reported text
size 13 and showed Size and Kind checked.

Three separate CLI runs established the catalog and execution boundaries:

| Run | Observed result |
| --- | --- |
| Menu source without `--wait-values` | Exit status 1 with an operation-signature rejection during compilation; the log contained no Effect poll or action delivery. |
| Read-only timeout source with `--wait-values` | `gui.select` followed by `gui.wait_value`; attempts one and two remained waiting, then the deadline expired with exit status 1 and no mutation. |
| Menu source with `--wait-values` | 11 Effects, one delivered menu choice, an initial `Waiting` result, then a matching `Some("16")` snapshot at revision 12; explicit acceptance let source return `true`, with 9,078 of the original 10,000 Fuel remaining. |

The successful source wait needed two predicate observations. Its first attempt
was nonmatching, but that sample's value was not printed; the record does not
identify it as 13. An ordinary observation before the menu choice did report
13. The expected optional value and 16-attempt/50-ms/2,000-ms budget came from
the `.lese` source, and the CLI used its generic wait driver.

A separate new CLI connection independently confirmed text size 16 and that
Size and Kind remained checked. Desktop screenshot inspection showed larger
text in the dedicated folder's main window; the exact value 16 came from the
independent AX connection, not from a screenshot or a View Options field read
by the computer-use tool. The previous dedicated Finder fixture and its
historical verification were left intact.

At this milestone, the Accessibility package passed 184 Rust tests, the
language workspace passed 2,303 tests and the independent Consumer passed 735,
all with zero failures and zero ignored tests. The 37 native boundary groups
also passed. Formatting and strict all-targets Clippy passed for the
Accessibility package, and the Rust embedding snippet above compiled to
metadata without executing native calls. This addition changed the Rust host,
catalog and CLI; it introduced no native bridge, HIR or VM changes.
