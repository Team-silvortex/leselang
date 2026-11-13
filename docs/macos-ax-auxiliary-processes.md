# Explicit Auxiliary Processes In A Widget Scope

Some macOS windows embed controls implemented by another process. For example,
Preview's Export panel can expose Accessibility objects owned by an AppKit
open/save-panel service. The widget adapter keeps its default same-process
scope and offers an explicit auxiliary-process grant for this case.

An auxiliary grant is supplied when creating a selected application's widget
connection. It admits specified running process instances only where their Accessibility
objects are reachable inside the selected native window. It does not select
another application, enumerate a helper's other windows or grant operating
system Accessibility permission. The existing `ApplicationAccess::widgets`
method and CLI commands without an auxiliary grant retain their same-process
behavior. The original controls, TextEdit and Calculator interfaces are
unchanged.

## Discover, Review And Grant

First select the running application and the exact window using the
[application guide](macos-ax-applications.md). The read-only
`--auxiliary-processes` action reports process identity candidates encountered
at that window's process boundaries. Discovery does not authorize those
processes and does not produce native object references for source to use.
An encountered foreign object is a boundary leaf: discovery reads its associated
PID and local process metadata, without requesting its AX properties or
traversing its descendants. The result is not an inventory of all helper
processes on the machine.

For a prepared Preview test window, the command has this form:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --window-title 'Your prepared test window' --auxiliary-processes
```

Use the actual window title and any additional exact identifier or document
selector needed to make the match unique. Each candidate contains a PID,
process start time in seconds and microseconds, and a canonical executable
path. Review that exact identity before passing it to widget discovery or a
source run:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --window-title 'Your prepared test window' --widgets --allow-auxiliary PID START_SECONDS START_MICROS '/absolute/path/to/reviewed/executable' --discover
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --window-title 'Your prepared test window' --widgets --allow-auxiliary PID START_SECONDS START_MICROS '/absolute/path/to/reviewed/executable' --run path/to/source.lese
```

The uppercase values above are placeholders for the current reviewed tuple.
Do not treat a previously recorded PID or process name as a reusable grant.
A service restart requires fresh discovery and an explicit new decision.
The CLI accepts at most four distinct auxiliary process tuples and permits
them only with `--widgets`. They are host configuration, not source-language
arguments or operations.

The native constructor checks that each process is owned by the current user,
that its executable matches the supplied canonical path, and that both parts
of its start time still match. The start time distinguishes a new process
that reuses an old PID. A same-named executable, a new instance at the same
path or a different process appearing under an old PID does not inherit the
grant. Path validation and process identity checks remain native checks even
when a tuple was previously returned by discovery.

## Rust Embedding And Revocation

[`scope`](../hosts/macos-ax/src/scope.rs) exposes `AuxiliaryProcessSpec` and
`AuxiliaryGrant`. `ApplicationAccess::auxiliary_processes` supplies discovery
metadata. The host decides which candidates to approve and constructs a
grant; discovery alone never makes that decision.

An embedder can pass an explicitly reviewed tuple to the native connector:

```rust
use leselang_macos_ax_host::{
    scope::{AuxiliaryGrant, AuxiliaryProcessSpec, WindowSelector},
    scope_access::ApplicationAccess,
    widgets,
};

fn run_with_auxiliary(
    application: &ApplicationAccess,
    window: &WindowSelector,
    approved: AuxiliaryProcessSpec,
    source: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let candidates = application.auxiliary_processes(window)?;
    if !candidates.contains(&approved) {
        return Err("the reviewed process is not a current window candidate".into());
    }
    let grant = AuxiliaryGrant::new(vec![approved])?;
    let controller = grant.clone();
    let backend = application.widgets_with_auxiliary(window, &grant)?;
    let completed = widgets::run(&backend, source)?;
    println!("{:?}", completed.value);
    controller.revoke();
    Ok(())
}
```

The example's candidate comparison is an early host check. The native
constructor and later operation boundaries still recheck the process; matching
a previously read tuple does not make the read/act sequence atomic.

Clones of a grant share one monotonic revocation state on the local thread.
`revoke()` returns true for the first revocation and false for later calls.
Revocation invalidates every widget connection using that grant. Reapproval
requires a new grant and a fresh native connection, with new execution-bound
references. The backend retains the grant while the native connection is live,
so dropping the caller's original grant value does not silently revoke or
invalidate its native callback storage.
`WidgetsAccess::revoke_auxiliary` offers the equivalent shared revocation from
an existing auxiliary connection and retires that native connection immediately.

For an interactive embedder, keep a grant clone alongside the explicit
`widgets::with_host` session driver. The host can revoke it between poll,
authorization, dispatch, pump and acceptance. A queued operation must then
refuse delivery; a reply awaiting acceptance must not regain authority.
Revocation cannot undo an operation already delivered to the application.
The synchronous `widgets::run` convenience driver does not install a scheduler
or a background grant watcher.

## Process Authority And Window Membership

A grant and native reachability are separate requirements. An object must
belong to the selected primary process or an explicitly granted auxiliary
instance, and it must remain in the selected window's bounded widget graph.
The primary application and exact native window stay pinned. Renaming a
window does not switch it; removing or replacing it invalidates the connection.
The adapter does not start traversal at an auxiliary application root.
For an `AXChildren` edge that crosses processes, the native reader checks that
the child's `AXParent` is the exact parent already encountered in this window.
It does not follow that parent upward to discover a helper's siblings. A shown
menu is admitted only through its scoped popup owner's supported relationship.

Process identity and host approval participate in permission checks, complete
scope refreshes and native delivery. A helper's exit, changed start time,
changed executable, changed user or revoked grant refuses further work. The
host does not search for another helper with the same name or executable.
Auxiliary elements use the same stable native identities, retirement rules,
observation revisions and typed item membership checks as primary elements.
Each visited object's owner PID is also retained as a native scope dependency,
including structural objects not exposed as language widgets. The reader checks
the current owner before reading object metadata and confirms it afterward.
Menu items and collection items cannot migrate between owners merely because
both processes appear in the grant.

The scope remains an Accessibility observation contract. It does not establish
a code-signing identity or prove an operating-system-authenticated relationship
between a helper and a particular application. A canonical executable path,
process instance and observed window relationships are the admitted evidence.
The external application can change after a final check, and changes that
return to their old state entirely between observations can remain unseen.

Apple documents `AXUIElementGetPid` as returning the process associated with an
Accessibility object. Its `AXChildren` attribute identifies immediate children,
and `AXParent` identifies the parent; `AXWindow` is a shortcut to an ancestor
window. These APIs supply relationship data rather than a transitive grant to
every object owned by a discovered PID. The adapter's explicit process list and
pinned-window traversal are its own authorization policy built on those APIs.
See Apple's [PID API](https://developer.apple.com/documentation/applicationservices/1460337-axuielementgetpid),
[child relationship](https://developer.apple.com/documentation/applicationservices/kaxchildrenattribute),
[parent relationship](https://developer.apple.com/documentation/applicationservices/kaxparentattribute)
and [window relationship](https://developer.apple.com/documentation/applicationservices/kaxwindowattribute?language=objc).

Apple's published [`proc_bsdinfo` definition](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/proc_info.h)
contains the process user and start-time fields, and its
[`libproc` implementation](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/libsyscall/wrappers/libproc/libproc.c)
provides the process-info and executable-path reads. These metadata checks do
not replace the executing host's existing Accessibility access. Apple's
[trust query](https://developer.apple.com/documentation/applicationservices/1460720-axisprocesstrusted?preferredLanguage=occ)
reports the current client's Accessibility trust; it does not grant permission
to a selected application or helper.

## Failures And Validation

The existing widget bounds and 13-operation catalog remain unchanged. Original
Fuel, execution lease, cancellation, stale snapshots, late replies and partial
side effects retain the behavior described in the
[widget guide](macos-ax-widgets.md). An error after entering a native mutation
remains an uncertain outcome, including a messaging timeout. Apple documents
that an action can time out while its callback performs modal work, so a
timeout is not evidence that the action never happened. The host does not
automatically retry it. See Apple's
[action API](https://developer.apple.com/documentation/applicationservices/1462091-axuielementperformaction?changes=_8).

Validation must distinguish process-policy tests, native boundary tests and a
real desktop source run. Regression cases include ungranted PIDs, wrong paths
or users, replaced process instances, grant revocation before delivery and
before acceptance, a granted process's unrelated window or item, parent and
menu ownership changes, and connection retirement. The default same-process
refusal remains a required test even when an explicitly granted workflow is
supported.

## Preview Export Reference

The [Preview source](../hosts/macos-ax/examples/widgets_preview_panel.lese)
exercises two verified edits to an actual helper-owned export-name field.
Prepare a dedicated PNG named `Panel Fixture.png`, open it in Preview and open
its Export panel through the ordinary desktop UI. Keep the panel's compact
layout and its initial export name `Panel Fixture`. The recorded fixture was
`target/auxiliary-fixture/Panel Fixture.png` in this repository. The source
edits the name field only; final export is a separate desktop action.

From the repository root, inspect the prepared document's exact window and
process boundary:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --windows
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --window-title 'Panel Fixture.png' --window-id PVDocumentWindow --window-document "$PWD/target/auxiliary-fixture/Panel Fixture.png" --auxiliary-processes
```

Review the returned PID, start seconds, start microseconds and executable path.
Use that complete current tuple in place of the placeholders below. A new CLI
invocation creates a new connection, but the tuple still identifies the same
already-running process instance; it is revalidated before access.

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --window-title 'Panel Fixture.png' --window-id PVDocumentWindow --window-document "$PWD/target/auxiliary-fixture/Panel Fixture.png" --widgets --allow-auxiliary PID START_SECONDS START_MICROS '/reviewed/canonical/executable' --discover
target/lean/leselang-macos-ax-host --app /System/Applications/Preview.app --window-title 'Panel Fixture.png' --window-id PVDocumentWindow --window-document "$PWD/target/auxiliary-fixture/Panel Fixture.png" --widgets --allow-auxiliary PID START_SECONDS START_MICROS '/reviewed/canonical/executable' --run hosts/macos-ax/examples/widgets_preview_panel.lese
```

Discovery should identify the editable `AXTextField` with identifier
`saveAsNameTextField`. The source obtains its native reference, verifies the
initial name, writes `LeseLang XPC draft` against the observation revision,
observes that exact value, writes `LeseLang XPC verified` against the new
revision and observes it again. Each later edit depends on the previous
postcondition. The successful final value is `LeseLang XPC verified`.

Independently inspect the actual Export panel, then choose Cancel to finish
this reference workflow without exporting a file. The name changes were real
delivered effects even though cancelling dismisses the temporary panel.

## Recorded Desktop Check

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
