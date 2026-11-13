# Discovering Controls In A Scoped macOS Application

The independent [`hosts/macos-ax`](../hosts/macos-ax/Cargo.toml) host has an
opt-in `--controls` mode for discovering and operating individual Accessibility
controls inside an explicitly selected window. Existing shortcuts scope that
window to a TextEdit document or Calculator; the
[application selection interface](macos-ax-applications.md) also accepts an
explicit running application bundle and exact window selector.
It uses the existing native object protocol and typed session driver. The
original TextEdit and Calculator profiles remain available without this flag;
their catalogs, default demonstrations and Fuel budgets are unchanged.

Menus, checkbox state, item selection and scrolling are available through the
separate optional [widgets catalog](macos-ax-widgets.md). The six-operation
controls catalog and its public data types remain unchanged.

This mode supports `AXButton`, `AXTextArea`, `AXTextField` and `AXStaticText`
targets. It discovers the actual controls in the selected window, including
buttons outside the original Calculator profile's four names. Discovery does
not expand authority beyond the host's selected application instance and window.
The [Accessibility host guide](macos-ax-host.md) covers building the host,
preparing a disposable TextEdit file and checking existing Accessibility access.

## Inspect Before Choosing A Control

Use one explicit application scope and request discovery:

```sh
target/lean/leselang-macos-ax-host --calculator --controls --discover
target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt --controls --discover
```

The application must already be open. TextEdit uses the exact canonical
document URL; Calculator uses its unique main window. The control mode does
not require the old Calculator profile's four button aliases or its English
Basic-mode labels. Actual roles, identifiers, labels and available actions
still depend on the application version and current layout. Inspect those
values before writing a selector, and handle missing or ambiguous controls.

`--discover` prints bounded metadata and performs no button press or text
replacement. It is a read-only inventory, not a LeseLang execution or a set of
exported capabilities. Printed strings escape control characters; this output
is intended for human inspection, not as a stable wire format.

Each discovery key belongs to that native connection. A new CLI invocation
creates a new connection, so a key copied from `--discover` is not a valid
handle for a later `--run`. Use the displayed role, identifier and label to
build an exact selector across runs. Within one execution, `gui.list` can
provide keys for `gui.find`. Neither keys nor selectors confer authority:
resolving them still requires a live authorized connection and creates a
fresh execution-bound native object reference.

`--controls` requires exactly one of `--discover` and `--run FILE`. It has no
implicit mutation demonstration. For example, create and open a fresh
disposable document as described in the host guide, then run:

```sh
target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt --controls --run hosts/macos-ax/examples/controls_textedit.lese
```

The [TextEdit control source](../hosts/macos-ax/examples/controls_textedit.lese)
selects the unique text area, checks the prepared initial text, queues a
replacement and requests a new observation. Its initial-content mismatch
branch performs no write; its final check distinguishes the expected replacement
from a submission that did not produce the intended value. TextEdit may autosave
that disposable document.
The [Calculator control source](../hosts/macos-ax/examples/controls_calculator.lese)
requires display `0`, selects the actual `Eight`, `Add`, `Seven` and `Equals`
buttons, and checks both final display `15` and last expression `8 + 7`.
This uses buttons outside the original four-button profile. It performs
17 Effects, including a fresh observation before each press and separate final
display/expression observations. An initial display mismatch performs no press;
a final mismatch returns `verification failed`. Its selectors describe a
particular layout, so inspect the live inventory and source before running:

```sh
target/lean/leselang-macos-ax-host --calculator --controls --run hosts/macos-ax/examples/controls_calculator.lese
```

The host does not clear or restore application state. Successful fake-backend
execution of an example does not establish that its selectors match a real
application version.

Source files must be UTF-8 and at most 16 KiB. Controls executions start with
10,000 Fuel and must return a scalar value. A native object or observation
cannot escape as the final result. Rejecting a final result does not roll back
previously delivered actions.

## Reuse A Task With Typed Inputs

The [text task](../hosts/macos-ax/examples/controls_text_task.lese) separates
application selection, task inputs and GUI logic. The same source discovers a
control, passes its native `object` reference into `replace_text`, observes it,
checks the expected text, submits a revision-checked edit and verifies a fresh
observation. A separate helper accepts the historical `snapshot` and projects
its optional text value. An unavailable value stays distinct from an empty
string.

For a fresh disposable TextEdit document prepared by this host:

```sh
target/lean/leselang-macos-ax-host --document /tmp/leselang-ax-demo.txt --controls \
  --run hosts/macos-ax/examples/controls_text_task.lese --entry main \
  --text role AXTextArea --text identifier '' --text label '' \
  --text expected $'LeseLang AX initial\n' --text text $'LeseLang reusable task ✓\n'
```

With an already-open application selected through `--app` and exact window
fields, the same file can target another supported text control. Supply the
role, identifier, label and initial value observed in that application. No
application name or document content is rewritten into the source. A successful
run returns `true` after four Effects. An initial-value mismatch returns `false`
after discovery and observation, with no edit. A successful submission followed
by a different value also returns `false`; treat only a final `true` as verified
success. Native errors and uncertain writes still fail without retry or rollback.

`--entry NAME` and repeatable `--text NAME VALUE` apply only to `--controls --run`.
Either option opts into task compilation; `--text` alone keeps the entry name
`main`. Without them, `--run` retains its parameterless `main` contract. At most
eight distinct input names are admitted, each a valid function-parameter name;
text is UTF-8 and at most 4,096 bytes per value. `--text` supplies a `string`,
with no parsing, interpolation or implicit conversion to another type. Missing,
extra, duplicate or incorrectly typed inputs fail before the first Effect.
The source compiler continues to validate all functions and cold branches.

Rust callers can use `controls::run_task(backend, source, entry, arguments)`
with an owned `Vec<(String, ScalarValue)>` for all six scalar types. Within an
explicit `with_host` scope, `Host::compile_task` exposes the compiled node,
inferred result type and required scalar entry parameters for direct VM
driving. `controls::run_task` uses the shared `embedding::bind_entry_arguments` validation and
the same original Fuel and explicit `poll → authorize → dispatch → accept`
driver as ordinary controls execution. Native `object` and `snapshot` types
are accepted only for helpers; they cannot be supplied as task inputs. Their
values still come from authenticated replies inside the selected session,
with the original object, revision and live-permission checks.

## Catalog And State

| Operation | Inputs | Result |
| --- | --- | --- |
| `gui.list` | `role: string` | Bounded `string_list` of current connection-local keys; an empty role includes all admitted roles |
| `gui.find` | `key: string` | Native reference for the uniquely resolved current control |
| `gui.select` | `role: string`, `identifier: string`, `label: string` | Native reference for the uniquely matching current control |
| `gui.observe` | `target` | Historical observation of that control and its scope-aware local revision |
| `gui.press` | `target`, `expected_revision: integer` | Acknowledgement that a supported button press was queued |
| `gui.set_text` | `target`, `expected_revision: integer`, `text: string` | Acknowledgement that a complete text replacement was queued |

The three `gui.select` fields are combined. A nonempty field must match exactly;
an empty field places no condition on that attribute. All three empty fields
are rejected. No match and multiple matches both fail. A selector never means
“take the first matching control”; refine it using metadata from discovery.
There is no substring matching, tree-position selector or implicit fallback
to a similarly named object. `gui.find` likewise rejects a key that is absent
or belongs to an earlier connection.

Each observation exposes these scalar fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `key` | `string` | Connection-local discovery identity, not a capability |
| `role` | `string` | Accessibility role |
| `identifier` | `string` | Application-supplied Accessibility identifier, possibly empty |
| `label` | `string` | Accessibility title, with a description fallback for buttons/static text; possibly empty |
| `value` | `optional_string` | Observed text value, or `none` when no string value is exposed |
| `enabled` | `boolean` | Enabled state admitted by the role policy below |
| `editable` | `boolean` | Whether the admitted text control supports text replacement |
| `pressable` | `boolean` | Whether the admitted button supports a press |
| `actions` | `string_list` | Supported operations, using `press` and `set_text` names |
| `revision` | `integer` | Local revision used to fence an action attempt |

An `AXStaticText` may be observed but is not editable. A button must expose
the native press action. Text replacement requires a settable text value on an
admitted text role. An action also requires the target to remain enabled.
When a text area or text field explicitly lacks `AXEnabled`, the native adapter
uses a successful value-settable check for its effective enabled state. An
explicit `AXEnabled: false` still blocks the action. Buttons without an enabled
attribute remain disabled. Attribute presence is retained in the scope epoch,
so a change between absence and an explicit state also fences queued work.
Metadata alone cannot be substituted for the `target` operand: observe and
action operations require the typed native reference returned by discovery.
Non-text nodes may expose numeric or boolean Accessibility values. Those stay
as typed internal scope dependencies instead of being coerced to text; their
public `value` is `none`. An admitted text role with a present non-string value
is rejected.

A read-only source can select the document's text area and project its value:

```text
fn main() = bind(
    target: gui.select(role: "AXTextArea", identifier: "", label: ""),
    body: bind(
        state: gui.observe(target: target),
        body: field(value: state, name: "value"),
    ),
)
```

If the scoped window contains multiple text areas, this source refuses its
ambiguous selector. For an action, first observe the selected target, pass
that observation's `revision` to the action, then request another observation
and check the intended postcondition. A queued acknowledgement is not that
postcondition.

## Identity And Scope Freshness

The native connection pins the process instance and selected window using the
same process start identity, document/window selection and Accessibility
element checks as the original profiles. Each admitted control receives a
distinct native identity and connection-local key. The registry compares
Accessibility elements by native equality, not array position or visible
text. Reordering the tree does not redirect an existing reference.

Removed controls are retired. Registry entries and keys are never reused in
that connection, including when a later control has the same role, identifier
and label. Replacing a control cannot revive its old native reference. A fresh
selector may resolve the replacement and issue new authority under the current
execution; the previous reference remains invalid.

Freshness includes the complete observed scope, not just the selected target's
attributes. The native scope epoch advances when the observed window tree or
its relevant metadata changes. In Calculator this includes display and
expression state, even when the selected button itself is unchanged. These
dependencies are retained even for tree nodes that are not exported as
operable targets. Scope changes invalidate an older queued action against any
affected control. The driver also consumes the local revision for each
admitted action attempt.

The observed projection includes roles, identifiers, titles, supported values,
enabled state, press/text capabilities and advertised attribute names, plus
every traversed element's native identity and parent edge. Buttons and static
text also contribute their descriptions; their public label uses a nonempty
title first and description otherwise. Text areas, text fields and containers
use titles without reading descriptions. This is a fixed role policy within
the adapter. Tree reordering can conservatively advance the epoch while preserving
control identity. This is a declared bounded projection of the application,
not a claim to observe every possible Accessibility attribute. Advertised names
are dependency metadata, not an exhaustive list of readable attributes: the
adapter queries its declared fields directly. Only explicit unsupported or
no-value results make an optional field absent. Other read failures remain
failures instead of being treated as missing values.
For a generic application/window connection, the selected window's document
metadata is also a scope dependency; a document-only change fences queued
work even when its title and control tree remain unchanged.

The host keeps the sequence explicit:

1. Poll the VM and authorize the current execution and Accessibility access.
2. Dispatch a read or queue an action against its expected revision.
3. Explicitly pump the action, rechecking permission, process/window identity,
   the target and the entire expected scope before entering the native call.
4. Accept the correlated reply, then let source request a new observation.

The same per-target result domain rejects an object or observation belonging
to another control, even if its scalar fields are identical. Execution leases,
cancellation, late replies and original Fuel continue through the shared host
driver. There is no background scheduler, implicit wait or automatic retry.

External snapshots still cannot detect a change from A to B and back to A
entirely between reads. The target process may also change after the final
read and before the native call. Scope epochs do not provide an atomic remote
compare-and-set operation. Once a native mutation has been entered, an error
or timeout has an uncertain outcome; earlier effects and possibly the current
effect may already have occurred. The adapter does not claim rollback.

## Bounds And Verification

The admitted inventory is limited to 64 live controls. The native registry and
local owner table retain at most 128 identities, including retired entries.
Exhaustion refuses work; starting another connection is an explicit new scope
with new keys and object references. The native walk also bounds windows and
children to 64 entries, the tree to 128 nodes and depth to 16, and each node's
advertised attribute list to 128 names. Strings are limited to 4,096 UTF-8
bytes; observed metadata, attribute names and keys share a 64 KiB bound. Unsupported
native types, excessive data and ambiguous scope selection fail before an
action. Bounds do not turn Fuel into preemption of native callbacks.

The portable [`ControlsBackend`](../hosts/macos-ax/src/controls.rs) supplies
bounded scope snapshots, permission checks and explicit press/text operations.
It uses the same host/session driver as the old profiles. A backend must retain
stable identity, advance its observed scope epoch and recheck the entire
expected scope immediately before mutation. Its post-entry errors must preserve
the uncertain-outcome classification. No platform framework is added to the
five language crates or independent Consumer.

`controls::discover` returns validated inventory metadata, `controls::run`
compiles and drives independent source with the controls budget, and
`controls::with_host` exposes the scoped host and execution scope for an embedder
that owns its explicit poll/authorize/dispatch/pump/accept loop.

Run the package, native boundary and independent Consumer checks described in
[CONTRIBUTING](../CONTRIBUTING.md). Failure coverage must include empty or
ambiguous selectors, stale keys, replaced and reordered controls, registry
exhaustion, cross-target replies, changes elsewhere in the scope, permission
revocation, cancellation before delivery, late replies, Fuel exhaustion and
partial effects. Portable fake backends and pure native tests establish these
local contracts; actual desktop acceptance additionally requires discovery,
source execution and a separate inspection of the real application's result.
The original profiles' recorded desktop runs do not establish a controls-mode
run by themselves.

## Recorded Desktop Checks

On 2026-11-13, controls mode completed a real Calculator run against
Calculator 12.0 on macOS 27.0.1, using its English Basic-mode layout. Read-only
discovery found 26 admitted controls with the display at `0`. Running the
external `controls_calculator.lese` source completed 17 Effects with 9,104 Fuel
remaining and returned `15`. The final native observations contained display
`15` and last expression `8 + 7`; a separate desktop inspection confirmed both
values in the actual Calculator window. This establishes that observed layout
and source path, not every Calculator mode or localization.

The same day's TextEdit controls run used the dedicated
`LeseLang-Controls-20261113.txt` document. Discovery found five admitted controls.
The external `controls_textedit.lese` source replaced `LeseLang AX initial\n`
with `LeseLang discovered controls ✓\n`, completed four Effects and retained
9,673 Fuel. Its observations advanced from local revision 0 to 3. A separate
desktop inspection confirmed the new text in that exact file's text area.
A subsequent source run on a new connection obtained its own key through
`gui.list`, passed that key to `gui.find` and observed the same resulting text.
That full key-resolution path completed three Effects with 9,774 Fuel remaining.

Three real negative checks also passed. A selector containing only
`role: "AXButton"` failed as ambiguous, and a key copied from an earlier
discovery connection failed to resolve in a new connection. Neither issued an
action. Repeating the Calculator source with display already at `15` stopped
after two read Effects, retained 9,881 Fuel and returned
`initial display mismatch` without pressing a button.

Read-only compatibility runs through the original profiles still worked:
Calculator returned last expression `8 + 7` through two Effects with 1,948 Fuel
remaining, and TextEdit returned the earlier dedicated test document's
`LeseLang cross-process ✓\n` value through two Effects with 860 Fuel remaining.

Before generic application/window selection was added, the Accessibility
package suite passed 70 tests: 57 library tests,
seven command-line tests, one external controls API test, two external TextEdit
API compatibility tests and three compile-fail ownership tests. Its 11 pure
native boundary groups also passed without remote GUI calls, alongside
formatting and strict Clippy across all package targets.

The independent language workspace's 2,303 tests, independent Consumer's
735 tests and AppKit package's five tests passed in those controls-mode checks, together
with their formatting and strict Clippy checks. These regression suites verify
the retained language and consumer boundaries separately from the real desktop
checks above.

The [application selection guide](macos-ax-applications.md) extends this catalog
to an explicitly selected running macOS bundle and window while preserving
the original scope shortcuts. Actual compatibility depends on each
application's Accessibility projection and bounds. Other platforms, complex
widget operations and a stable remote wire format remain separate work.

### One Typed Task Across TextEdit And Stickies

On 2026-11-13, both applications ran the same unchanged
[`controls_text_task.lese`](../hosts/macos-ax/examples/controls_text_task.lese)
with scalar inputs supplied through `--text`. The source SHA-256 was
`b067212732a0242ea32f576b12c9785da82a76afdc68525f0dda047362bee7a8`.
Both runs selected `AXTextArea` with an empty label; the application/window
scope, observed identifier, expected text and replacement text were host inputs.

| Application and target | Initial text | Verified replacement | Effects | Final value | Fuel remaining |
| --- | --- | --- | --- | --- | --- |
| TextEdit, dedicated `/tmp/leselang-task-inputs-textedit-20261113.txt`, identifier `First Text View` | `LeseLang AX initial\n` | `LeseLang reusable task: TextEdit ✓\n` | 4 | `true` | 9,555 / 10,000 |
| Stickies, dedicated note, identifier `_NS:153` | `LeseLang task initial 20261113\n` | `LeseLang reusable task: Stickies ✓\n` | 4 | `true` | 9,568 / 10,000 |

The Stickies scope was selected by its exact `.rtfd` document path. Its title
changed with the edited first line without changing the pinned scope. The note
was created through the application's UI: attempting to open a plain-text file
had produced an unsupported-document dialog, which was closed before creating
the dedicated note. Opening that plain-text file was not a successful fixture
preparation step.

A subsequent Stickies run supplied the old expected text and requested
`This must not be written\n`. It completed only the two read Effects, returned
`false`, retained 9,812 Fuel and performed no mutation. Its process exit status
was zero because evaluation completed; the `false` result correctly signaled
that the task's condition was not satisfied.

After execution, new native connections independently discovered both final
text values, and a separate desktop inspection read the text in each actual
application. Stickies still contained its verified replacement after the
negative run. The sandboxed permission check reported no Accessibility access;
a non-prompting check outside that sandbox found existing access.
The verification did not change system permissions. These runs establish the
observed TextEdit and Stickies workflows, including native helper parameters
and scalar task inputs; they do not establish arbitrary application support.
