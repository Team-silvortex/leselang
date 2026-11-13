# Menus, Checks, Selection And Scrolling

The independent macOS Accessibility host has an optional `--widgets` catalog
for common controls beyond buttons and text editors. It uses the same explicit
application/window selection and native session engine as the existing
`--controls` catalog. The original control data types, backend interface and
entry points remain available.

Select an already-running application and an exact window using the
[application guide](macos-ax-applications.md), then inspect the widget inventory:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/TextEdit.app --window-title 'Your test window' --widgets --discover
```

The title is supplied by the application. Use actual metadata from `--windows`
rather than treating this illustrative title as a built-in alias. Widget mode
requires an explicit discovery or source-run action; it does not run a default
demonstration. Generic application and exact window scope remain host decisions
made before LeseLang source runs.

## Explicit Operations And Verification

Widget mode retains `gui.list`, `gui.find`, `gui.select`, `gui.observe`,
`gui.press` and `gui.set_text`. Its 13-operation catalog adds exact value-based
discovery and operations for opening and choosing a scoped popup menu, cancelling
that menu, setting a checkbox to a desired state, selecting an actual list item
and moving a scrollbar to an explicit position.

`gui.select_value(role, value)` finds one widget by its exact nonempty role and
current string value. An empty value matches an actual empty string, not an
absent value. Zero matches and multiple matches fail; disabled matches still
participate in ambiguity checks. The result is the same execution-bound native
reference returned by other discovery operations. Finding a disabled control
does not enable it, and value-based lookup grants no mutation authority.
The original `--controls` catalog does not gain this operation.

This is useful when a collection exposes a file name as the value of a nested
text field. Source can observe the field, resolve its `parent_key` through
`gui.find`, verify each expected role and relationship, and reach the actual
row before requesting selection. The parent key remains connection-local
discovery metadata; native membership is checked again before delivery.

Every mutation uses a native target reference and an observation revision.
Discovery keys and labels are lookup metadata, not arguments that can replace
those references. The source should observe the intended target, request the
operation using that observation's revision, then obtain a new observation and
check the intended result. A successful submission or native acknowledgement
does not establish the application's postcondition.

Checkbox operations request an explicit checked or unchecked state rather than
an unconditional toggle. A control already in that state requires no native
press. A mixed or otherwise unsupported checkbox state must not be silently
treated as false. The adapter observes the actual value and checks the native
capability again before delivering any required press.

List selection refers to a discovered item with its current native identity.
The adapter checks that the item remains in the selected widget tree and that
its selection capability is still supported. Array positions, text matches
and indices do not authorize selecting a replacement item. Selection is a
single explicit operation; it does not also activate or open that item.
When a container exposes a writable selected-items attribute, the adapter sets
that attribute to the requested item. Otherwise, a supported item can be
selected by setting its own `AXSelected` state. That latter path need not clear
other selections. Inspect the container's `selected_keys` and the individual
items' available selection states when the postcondition requires exclusivity.

Scrolling uses an explicit scrollbar position from 0 through 1,000. The native
adapter maps that normalized integer to the scrollbar's observed finite range
and requires a settable value. A scrollbar that omits both range endpoints uses
the standard normalized `AXValue` range of 0 through 1; one missing endpoint or
an invalid numeric range is refused. This does not synthesize wheel events, drag a
thumb by screen coordinates or repeatedly scroll until some text appears.
The application may clamp or interpret the requested position, so verification
requires a separate observation of the resulting state.

The seven additional operations have these inputs:

| Operation | Inputs | Meaning |
| --- | --- | --- |
| `gui.select_value` | `role: string`, `value: string` | Find one current widget with that exact role and string value |
| `gui.open_menu` | `target`, `expected_revision` | Open the supported menu of the selected owner |
| `gui.choose_menu_item` | `target`, `expected_revision`, `item` | Choose a current native menu item under that stable owner |
| `gui.cancel_menu` | `target`, `expected_revision` | Cancel the owner's current supported menu |
| `gui.set_checked` | `target`, `expected_revision`, `checked: boolean` | Request an explicit checkbox state |
| `gui.select_item` | `target`, `expected_revision`, `item` | Select a current native item in the target list, table or outline |
| `gui.scroll` | `target`, `expected_revision`, `position: integer` | Set a scrollbar position from 0 through 1,000 |

For menu choice and list selection, `target` is the stable owner and `item` is
a native reference returned by widget discovery. Both must belong to the same
live execution and the expected current scope. The item is resolved and checked
again before delivery. The operation cannot choose an item from another owner
merely because its label or displayed value matches.

Alongside the existing identity, text, capability and revision fields, widget
observations expose:

| Field | Type | Meaning |
| --- | --- | --- |
| `check_state` | `string` | `unavailable`, `unchecked`, `checked` or `mixed` |
| `selected` | `boolean` | Observed selection state when selection information is available |
| `selection_available` | `boolean` | Whether the selection state is supported by the observed item |
| `selected_keys` | `string_list` | Connection-local keys for the container's observed selected items |
| `parent_key` | `optional_string` | Discovery metadata for the observed parent relationship |
| `position` | `integer` | Normalized scrollbar position; zero when unavailable |
| `position_available` | `boolean` | Whether the scrollbar position is supported by the observation |
| `orientation` | `string` | `vertical`, `horizontal`, or empty when unavailable |

Use the observation's `actions` list to distinguish an observable value from an
available mutation. Rust embedders can also inspect `WidgetCapabilities`.
`selected: false` alone does not
mean that selection is supported, and `position: 0` alone does not establish
the beginning of a supported scroll range. A normalized scroll position is a bounded
projection of native numeric state; the native scope still retains the relevant
range and value dependencies for its immediate preflight checks.
`selected_keys` contains the directly owned admitted items observed as selected.
An empty list does not establish that every item's selection state is available.

## Menus Stay Within The Selected Scope

Menu discovery follows a popup or menu-owning control that belongs to the
selected window. It does not implicitly add the application's global menu bar
or another process's menus to the authorized scope. Opening a menu and choosing
an item remain separate Effects. This allows source to inspect the actual
available items and reject absent, ambiguous or disabled choices.

A menu item may disappear immediately after it is chosen. The operation must
retain correlation with its stable menu owner while validating the item's
native identity and current membership before delivery. A newly appearing item
with the same label does not inherit an earlier item's authority. Menu
membership and relevant state changes participate in scope freshness.

Menus may be transient, lazily populated or inaccessible through the supported
AX relationships. If the adapter cannot establish the required bounded state
and ownership, it refuses the operation. There is no fallback to the focused
menu, a matching title elsewhere, keyboard shortcuts or screen coordinates.

Native acknowledgement may precede the application updating its displayed
value. A failed immediate postcondition must stop dependent mutations. The
host may explicitly start another read to check the resulting state later;
that does not require resubmitting the earlier mutation or introducing an
implicit scheduler. The Finder reference below exercises this distinction.

By default, every admitted AX element must belong to the selected process. A dialog
that appears inside an application's window may contain Accessibility objects
owned by an AppKit XPC helper. Those foreign-process children are refused; the
adapter does not automatically add that helper process to the authorized scope.
An embedder can opt into a separate, explicitly revocable
[auxiliary-process grant](macos-ax-auxiliary-processes.md). That grant identifies
particular running processes and admits only their objects reachable within
the selected window; it does not add helper-window discovery to the language.

## Rust Embedding And Bounds

The additive portable API is in [`widgets`](../hosts/macos-ax/src/widgets.rs).
`WidgetInfo` wraps the original `ControlInfo` and adds typed state, parent
metadata and `WidgetCapabilities`. `WidgetScope` validates a complete bounded
snapshot. A `WidgetsBackend` provides `snapshot`, `check_permission` and
`perform` with a typed `WidgetAction`; backend item IDs are resolved by the host
from native references before delivery.

On macOS, `ApplicationAccess::widgets` creates a thread-affine `WidgetsAccess`
for the selected live window. For example, an embedder can accept an explicit
bundle path, window selector and source:

```rust
use leselang_macos_ax_host::{
    scope::{ApplicationSpec, WindowSelector},
    scope_access::ApplicationAccess,
    widgets,
};
use std::path::PathBuf;

fn run_widgets(
    bundle_path: PathBuf,
    selector: WindowSelector,
    source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let application = ApplicationAccess::connect(&ApplicationSpec {
        bundle_path,
        pid: None,
    })?;
    let backend = application.widgets(&selector)?;
    let inventory = widgets::discover(&backend)?;
    println!("{} admitted widgets", inventory.widgets.len());
    let completed = widgets::run(&backend, source)?;
    println!("{:?}", completed.value);
    Ok(())
}
```

`widgets::with_host` exposes the catalog and execution scope for embedders that
drive the shared session explicitly. `widgets::run` is a synchronous reference
driver with 10,000 Fuel. The native connection cannot be transferred to another
thread. The old `ControlsAccess`, `ControlsBackend`, control structs and six
operation catalog remain separate and retain their earlier bounds.

Widget traversal visits at most 512 native nodes with depth at most 24; each
native children or selection array is bounded to 64 elements. It admits at
most 64 widgets and retains at most 128 widget identities, including retired
ones. Each projected string is bounded to 4,096 UTF-8 bytes, all discovery keys
together fit within the same 4,096-byte scalar string-list bound, and the
complete observed native metadata shares a 64 KiB budget. Exceeding a bound
refuses the scope instead of silently truncating it. Increasing traversal
space does not increase the language's string-list bounds.

Alongside the old button and text roles, the native projection admits checkboxes,
popup/menu owners, menus and menu items, lists/tables/outlines, rows/cells and
scroll areas/bars. An `AXGroup` is admitted only as an immediate `AXList` child
with an established selection state. Table and outline selection admits
`AXRow` items; headers and their buttons do not acquire synthetic selection
state. Unadmitted structural ancestors remain
freshness dependencies. Visible labels are not substituted for the actual
selectable item, and the tree need not expose every visual control in a usable
form.

## Session And Failure Semantics

The shared driver keeps the sequence explicit:

1. Poll and authorize the current execution and native scope.
2. Dispatch a read or queue a mutation against the observed revision.
3. Pump the queued mutation, checking permission, process/window identity,
   control identity, capabilities, membership and the complete expected scope.
4. Accept the correlated reply, then let source request its result observation.

No scheduler, implicit wait, automatic retry or rollback is introduced.
Cancellation before delivery prevents the queued operation. Permission
revocation, stale observations and exhausted original Fuel refuse delivery.
An error after entering a native mutation preserves the uncertain-outcome
classification: the current action and earlier side effects may already have
occurred. A later postcondition failure does not undo those effects.

The adapter's remote freshness remains observation-based. Another application
can change after the final check, and a change that returns to the same value
entirely between observations cannot always be detected. Native bounds do not
provide atomic remote compare-and-set or preemption of native callbacks.

Use dedicated test content for desktop acceptance. A native dialog can change
selection, checkbox state or format while still requiring a separate final
save, open or print action. The reference workflow should verify the requested
widget state and explicitly decide whether that final action is part of its
scope. Cancelling a dialog is not evidence that its earlier widget operations
were never delivered.

## Validation

Run the package, native boundary and independent Consumer checks in
[CONTRIBUTING](../CONTRIBUTING.md). Widget-specific failures include mixed or
invalid checkbox values, unsupported or disabled controls, menu membership
changes, foreign items, selection replacement, invalid scrollbar ranges,
out-of-range requested positions, stale revisions, permission revocation,
cancellation, late replies, original Fuel exhaustion and partial effects.

Portable fake backends establish language/session behavior. Pure native tests
establish bounded type decoding, membership checks and action admission
without operating a desktop application. Real acceptance additionally requires
an actual application source run and an independent inspection of the resulting
UI. The prior TextEdit, Calculator and Stickies checks do not establish the new
widget operations by themselves.

## Finder Reference Workflow

The reference uses Finder's own folder window and its separate View Options
window. Each phase explicitly selects one window before starting its session;
source does not switch windows. The two windows can have the same title, so the
commands combine that title with the reported identifiers `FinderWindow` and
`ViewOptions`.

Prepare a dedicated folder named `LeseLang Widget Folders` containing 12 empty
subdirectories named `Widget folder 01` through `Widget folder 12`. The recorded
check used `/tmp/LeseLang Widget Folders`. Open that folder in Finder's list
view, hide the sidebar and sort Name in ascending order. In its View Options,
set text size to 13, choose the large icon size, enable the Size and Kind
columns, and disable the other optional columns, including Date Modified.
These are settings for this dedicated folder; leave **Use as Defaults** unused.

Build the host using the [host guide](macos-ax-host.md), then inspect the current
windows and View Options inventory:

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --windows
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'LeseLang Widget Folders' --window-id ViewOptions --widgets --discover
```

Use the actual metadata if the application localizes its labels or reports
different titles. The reference expects the popup identifier `Text size:`,
menu item label `16`, and checkbox labels `Size` and `Kind`. View Options
discovery admitted 30 widgets in the tested layout. The main folder window can
exceed the 64-widget limit while all its columns are visible, so finish the
options phase before discovering that window.

Run [the menu source](../hosts/macos-ax/examples/widgets_finder_options.lese):

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'LeseLang Widget Folders' --window-id ViewOptions --widgets --run hosts/macos-ax/examples/widgets_finder_options.lese
```

It requires an initial value of 13, opens and cancels the popup, checks that
the value remains 13, opens it again, chooses the native item labeled 16 and
observes the owner. If this last observation still reports 13, the program
returns `false` and stops. Inspect the value through a later explicit read,
such as another `--discover`, before continuing. Do not rerun the menu mutation
to compensate for a delayed observation.

Once a new read confirms 16, run
[the checkbox source](../hosts/macos-ax/examples/widgets_finder_columns.lese):

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'LeseLang Widget Folders' --window-id ViewOptions --widgets --run hosts/macos-ax/examples/widgets_finder_columns.lese
```

This new session first verifies text size 16, requests `checked: false` for
Size and observes `unchecked`, then does the same for Kind. A failed observation
stops the dependent operation. A successful result is `true`, and the main
folder window now has only the Name column.

Close View Options through the ordinary desktop UI and leave all rows
unselected. Make the folder window short enough that its 12 large rows require
vertical scrolling, and wide enough that no horizontal scrollbar is needed.
Start at the top of the list. Inspect the `FinderWindow` inventory, then run
[the selection and scrolling source](../hosts/macos-ax/examples/widgets_finder_selection.lese):

```sh
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'LeseLang Widget Folders' --window-id FinderWindow --widgets --discover
target/lean/leselang-macos-ax-host --app /System/Library/CoreServices/Finder.app --window-title 'LeseLang Widget Folders' --window-id FinderWindow --widgets --run hosts/macos-ax/examples/widgets_finder_selection.lese
```

The source finds the exact `AXTextField` value `Widget folder 01`, follows and
verifies its `AXCell` and `AXRow` ancestors, and checks the row's membership in
the `AXOutline` identified as `ListView`. It requires that row to be unselected
and the outline's `selected_keys` to be empty. After selection, it verifies both
the row's selected state and that the outline has exactly that one selected
key. It then requires one enabled vertical scrollbar at position 0, sets it to
1,000 and independently observes position 1,000. The final result is `true`
only when all these postconditions hold. Selection does not open the folder.

## Recorded Desktop Check

On 2026-11-13, this workflow ran against Finder 27.0 on macOS 27.0.1 with existing
Accessibility access and dedicated temporary folders. The initial options
phase used five folders; the fixture was expanded to 12 after hiding Size and
Kind and choosing large icons. A reproduction can prepare all 12 first, as
long as it completes the View Options phase before discovering the main window.

| Source and window | Effects | Fuel remaining | Observed result |
| --- | --- | --- | --- |
| Original guarded menu phase, `ViewOptions` | 11 | 9,340 | Open and cancel succeeded; choosing 16 was acknowledged, but the immediate owner observation remained 13, so source returned `false` |
| `widgets_finder_columns.lese`, a new `ViewOptions` session | 10 | 9,359 | A fresh observation confirmed 16; Size and Kind each became `unchecked`, and source returned `true` |
| `widgets_finder_selection.lese`, `FinderWindow` | 15 | 8,871 | The exact row became the sole selected item; scrollbar position changed from 0 to 1,000, and source returned `true` |

The menu phase was subsequently extracted into `widgets_finder_options.lese`.
Its original source had guarded follow-up branches that were not reached after
the false result. The recorded menu Fuel belongs to that original source; the
current standalone example preserves the 11-Effect behavior but can have a
slightly different Fuel cost. The other two rows used the current source flows.

The menu result establishes the distinction between delivery and the observed
postcondition. A separate later read confirmed 16 before the checkbox session;
the chosen menu action was not retried. No scheduler or waiting loop was added
to the host to make the immediate check pass.

Independent desktop inspection confirmed that row 01 and its cell/text field
were selected, Finder reported `1 of 12 selected`, and the native scrollbar
value was 1, corresponding to normalized position 1,000. A screenshot showed
the bottom of the list with folder 12, the large icons and changed text size,
and the absence of Size and Kind columns. These checks verify this Finder
layout and the explicit two-window workflow.

The same development pass tested Preview's Export panel and refused it because
some children belonged to the AppKit open/save-panel XPC process rather than
Preview's pinned PID. That panel remains outside the default same-process
widget scope; the separate [auxiliary-process guide](macos-ax-auxiliary-processes.md)
describes explicit opt-in authorization. Finder also returned its desktop `AXScrollArea` in `AXWindows`;
the inventory now validates native role metadata and admits actual `AXWindow`
objects only. Neither case broadens the selected process or window authority.

At this Finder milestone, the Accessibility package passed 123 Rust tests, including direct
reference-source tests for the delayed menu observation and stopping after a
failed checkbox postcondition. Its 30 native boundary groups passed without
remote GUI calls. The language workspace's 2,303 tests, independent Consumer's
735 tests and AppKit host's five tests passed, and formatting and strict Clippy
passed across all four workspaces. The Rust embedding example in this guide
also compiled without executing native calls.
