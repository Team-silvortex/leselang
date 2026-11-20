# macOS Accessibility Acceptance Records

These dated records retain their original application, platform, source and test
scope. Historical counts describe the named milestone, not the current suite.
Follow the linked source guides for current setup and contracts; see the
[acceptance index](README.md) for other components. Paths under `target/` identify
local ignored evidence and are not distributed artifacts. References within a
record to “this guide” or “above” retain the context of its linked source guide.

## TextEdit And Calculator Profiles

Source guide: [macos-ax-host.md](../macos-ax-host.md).

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
cases. The [external compatibility check](../../hosts/macos-ax/tests/textedit_api.rs)
also preserves unannotated TextEdit
reply, native-value and empty run-summary construction alongside the existing
backend and snapshot interfaces. Strict Clippy, formatting checks and the six
pure native boundary groups passed. These results complement the real runs;
deterministic tests cover failure paths without manipulating other applications
or changing OS permissions.

## Control Discovery

Source guide: [macos-ax-controls.md](../macos-ax-controls.md).

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

## Shared Typed Task

Source guide: [macos-ax-controls.md](../macos-ax-controls.md).

On 2026-11-13, both applications ran the same unchanged
[`controls_text_task.lese`](../../hosts/macos-ax/examples/controls_text_task.lese)
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

## Application And Window Selection

Source guide: [macos-ax-applications.md](../macos-ax-applications.md).

On 2026-11-13, the generic application/window path completed a real flow against
Stickies 10.3 on the same macOS 27.0.1 desktop used for the earlier adapter
checks. The host selected `/System/Applications/Stickies.app` and listed three
windows without reading their control contents. The dedicated note was chosen
by its exact reported title, `LeseLang scope initial 20261113…`; control
discovery admitted four controls, including its writable text area. The
application's window identifier was shared by multiple windows, so it was not
used as a unique selector.

Running the external `controls_stickies.lese` source completed six Effects
with 9,493 Fuel remaining and returned `LeseLang generic app window ✓\n`.
The three observations contained the expected initial, intermediate and final
values under the same control identity, with local revisions 0, 3 and 6. The
second text delivery succeeded after the first delivery had changed the note's
content-derived title. This exercised the retained native window rather than
repeating the original title lookup.

A separate desktop inspection confirmed the final text in the same window and
document, whose title had become `LeseLang generic app window ✓…`. The run used
a dedicated test note and existing Accessibility access. These observations
establish this tested Stickies layout and source flow; other application
layouts still require their own compatibility checks.

Real refusal checks also passed. Selecting the shared window identifier
`_NS:87` failed as ambiguous. After the note changed title, a new connection
using the original exact title failed to find a match. Specifying PID `1`
alongside the Stickies bundle failed without falling back to another process.
These commands issued no control action. A separate read-only source selected
the note by its exact local `.rtfd` package-directory document path and
returned the final text through two Effects with 9,881 Fuel remaining. This
also verified packaged-document selection independently of the changed title.

The final Accessibility package suite passed 86 tests, including the new
selector admission, FFI decoding, command-line, external API and thread-affinity
checks. Its 18 pure native boundary groups passed without remote GUI calls.
Those groups exercise exact application matching, bounded window metadata,
unique combined selectors, connection-local keys, title changes, reordering,
replacement, tombstones, document dependencies and publication refusal. The
shared process-start and permission guards remain part of the transport;
mocked selector tests do not replace desktop permission and process checks.

The language workspace's 2,303 tests, independent Consumer's 735 tests and
AppKit host's five tests also passed. Formatting and strict Clippy across all
targets passed for all four workspaces. The Rust example in this guide compiled
against the host's public API without executing native calls. These checks preserve the
language and consumer boundaries separately from the Stickies desktop result.

## Widgets

Source guide: [macos-ax-widgets.md](../macos-ax-widgets.md).

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
widget scope; the separate [auxiliary-process guide](../macos-ax-auxiliary-processes.md)
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

## Auxiliary Process Grants

Source guide: [macos-ax-auxiliary-processes.md](../macos-ax-auxiliary-processes.md).

On 2026-11-13, Preview 11.0 on macOS 27.0.1 exposed the prepared Export panel
inside its `PVDocumentWindow` titled `Panel Fixture.png`. The primary process
was PID 88883. Read-only boundary discovery reported one helper, PID 3538, with
start seconds `1794551094`, start microseconds `584858` and this executable:

```text
/System/Library/Frameworks/AppKit.framework/Versions/C/XPCServices/com.apple.appkit.xpc.openAndSavePanelService.xpc/Contents/MacOS/com.apple.appkit.xpc.openAndSavePanelService
```

These values record the tested instance; they are not reusable setup values.
With that exact explicit grant, widget discovery admitted 29 widgets. The
cross-process `AXParent` check matched the already visited parent without an
exception or alternate lookup path. The same widget discovery without a grant
refused the foreign PID. A grant with start seconds increased by one also
failed, with no fallback to the currently running service.

Running the repository's `widgets_preview_panel.lese` completed six Effects
with 9,472 Fuel remaining. Its three observations matched `Panel Fixture`,
`LeseLang XPC draft` and `LeseLang XPC verified`, and the returned string matched
the final value. Independent Accessibility inspection and a screenshot
confirmed that final name in the real Export panel. The panel was then
cancelled and the original `Panel Fixture.png` document remained open; no
export was saved.

This desktop check establishes discovery, explicit helper authorization,
native input-field edits and result verification for this Preview layout.

The Accessibility package passed 136 Rust tests, including auxiliary-grant
input validation, shared revocation, dispatch/delivery/acceptance fences,
targetless replies, callback lifetime and the actual Preview reference source.
Its 37 pure native boundary groups passed without remote GUI calls. The new
native groups cover process identity changes, typed boundary discovery without
foreign AX property reads, cross-process parent checks, thread constraints,
revocation and owner-PID observation dependencies.
The language workspace's 2,303 tests, independent Consumer's 735 tests and
AppKit host's five tests also passed. The host's final build and strict Clang
checks passed, as did formatting and strict Clippy. The Rust embedding example
in this guide compiled against the public API without executing native calls.

## Host-Driven Observation Waiting

Source guide: [macos-ax-observation-wait.md](../macos-ax-observation-wait.md).

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

## Source Value Waiting

Source guide: [macos-ax-source-waits.md](../macos-ax-source-waits.md).

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

## Source State Waiting

Source guide: [macos-ax-state-waits.md](../macos-ax-state-waits.md).

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
