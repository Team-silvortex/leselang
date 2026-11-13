# Selecting A macOS Application And Window

The independent [`hosts/macos-ax`](../hosts/macos-ax/Cargo.toml) adapter can
connect its existing controls catalog to an explicitly selected application
bundle and window. The application supplies ordinary macOS Accessibility
objects; it does not need a LeseLang-specific integration. The original
TextEdit and Calculator entry points remain available.

Application and window selection happen in the Rust host before source runs.
The resulting `ControlsAccess` implements the same `ControlsBackend` used by
the [control discovery guide](macos-ax-controls.md). Source can discover and
operate the admitted controls inside that window through the existing six
operations. It cannot select a different application or window by issuing a
control Effect.

The same explicit scope can supply the optional
[widgets catalog](macos-ax-widgets.md) for popup menus, checkboxes, selection and
scrollbars. This keeps application/window admission separate from the widget
operations a source program may issue inside that scope.
An optional [auxiliary-process grant](macos-ax-auxiliary-processes.md) can admit
explicit running helper instances for widgets reachable from that same window.
The application's window inventory remains scoped to the primary process.

This is an explicit scope-selection interface, not a claim that every macOS
application exposes a usable or sufficiently small Accessibility tree. The
adapter retains the documented role, metadata and traversal limits, and fails
when it cannot obtain the required bounded state.

## Select A Running Application Instance

Build and check existing Accessibility access using the commands in the
[host guide](macos-ax-host.md). Open the intended application through its
ordinary desktop workflow. The adapter does not launch applications, move
focus, grant permissions or choose the frontmost application.

Use an absolute `.app` bundle path to inspect the application's window
metadata. For example, after opening Stickies:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Stickies.app --windows
```

The bundle path identifies the executable to which the host may connect. The
native adapter resolves the bundle and executable, then matches a running
process owned by the current user. It pins that process's PID, start identity
and executable path. A matching display name or bundle identifier in a
different bundle does not substitute for the requested path.

If multiple matching instances are running, provide `--pid PID` alongside
the same `--app` path. The PID narrows the selected bundle's instances; it does
not authorize an unrelated executable or another user's process. A PID alone
is not an application selector. Process exit, restart or reuse of the same
PID fails the pinned connection's later checks.

The `--windows` action only reports bounded metadata for that selected
application. It does not traverse each window's controls or issue a native
mutation. Output includes the selected process and each window's connection
key, title, identifier and optional document URL. Printed strings escape
control characters. This is human-readable discovery output, not a stable
serialization of native handles.
Some applications include non-window Accessibility objects in `AXWindows`.
The adapter checks each entry's native process and role, then projects actual
`AXWindow` objects; Finder's desktop `AXScrollArea` is not a selectable window.
A failed or invalid role read still refuses discovery.

## Select One Window

Use one or more exact window metadata fields to refine the intended window:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Stickies.app --window-title 'LeseLang scope initial 20261113…' --controls --discover
```

The title above is from the dedicated reference note, including the ellipsis
reported by Stickies. It is not a built-in alias or a substring selector.
Create your own dedicated test note and use its actual title from `--windows`;
an application can derive or shorten its window title differently from its
editable text. Available selectors are:

| Selector | Meaning |
| --- | --- |
| `--window-title TITLE` | Exact Accessibility window title |
| `--window-id ID` | Exact application-supplied Accessibility identifier |
| `--window-document PATH` | Exact canonical local file URL derived from the supplied document path |

Supplied fields are combined. At least one window field is required for
generic `--controls --discover` or `--controls --run FILE`. No match and
multiple matches both fail; the adapter never takes the first window or falls
back to its position, focus or a similar title. A document selector is for an
existing local filesystem document, not an arbitrary web URL. It may name a
regular file or a package directory such as a Stickies `.rtfd` document. An
application's absent identifier or document attribute does not make a broader
selector unique.

Once selected, the connection retains the native window identity. Later
observations check that this same window remains in the pinned application's
window list. They do not rerun the original title selector and switch to a
different match. This lets a selected note's title change when its content
changes while preserving its identity. If the window disappears or is
replaced, an identically titled new window does not inherit its authority.
The exact metadata fields describe the initial lookup; the generic connection
then authorizes that window identity. This differs from the original TextEdit
document profile, which continues checking its configured document URL.
After that connection ends, a new invocation performs a new lookup. If the
previous operation changed the title, inspect the current window metadata and
use its new exact title or another unique field; the old title is not an alias.

The read-only `--discover` action now enumerates controls within that selected
window. Use its actual role, identifier and label metadata to write source.
Run that source with the same application and exact window-selection fields:

```sh
target/lean/leselang-macos-ax-host --app /System/Applications/Stickies.app --window-title 'LeseLang scope initial 20261113…' --controls --run hosts/macos-ax/examples/controls_stickies.lese
```

The repository's [Stickies source](../hosts/macos-ax/examples/controls_stickies.lese)
expects a dedicated note containing exactly `LeseLang scope initial 20261113\n`.
It selects the unique text area, verifies that initial content, writes
`LeseLang scope intermediate\n`, observes that intermediate result, then writes
and verifies `LeseLang generic app window ✓\n`. This six-Effect path exercises
two deliveries through the same selected window while its content-derived
title can change. An initial mismatch performs no write; an intermediate
verification failure prevents the second write. Use this source only after
preparing that test note and selecting its actual window metadata.
Stickies manages note persistence, so prepare disposable test content.

There is no default source or mutation for a generic application. The command
requires an explicit `--windows` action, or a window selector plus exactly one
of `--discover` and `--run`. Generic application arguments cannot be combined
with the original `--document` or `--calculator` scope shortcuts. The
`--window-document` flag selects a window inside the explicitly named app;
the original `--document` flag continues to select the TextEdit profile.

## Connection Keys And Rust Embedding

Window keys and control keys both belong to their native connection. A new CLI
invocation creates a new connection, so a key printed by `--windows` cannot be
copied into a later invocation. The CLI therefore exposes exact metadata
selectors rather than a cross-run window-key flag.
If a window exposes no usable nonempty title, identifier or local document,
the same-connection Rust key API still allows the host to select it explicitly.

An embedder can keep an `ApplicationAccess` alive, inspect its `windows()`
inventory and pass a chosen key back to the same connection. Window keys are
never recycled after removal. A key from a different connection, including
one for the same application, fails. Keys remain metadata: resolving one
requires live process and permission checks before it produces a pinned
controls connection.

The public types are in [`scope`](../hosts/macos-ax/src/scope.rs), and the
macOS transport is in [`scope_access`](../hosts/macos-ax/src/scope_access.rs):

```rust
use leselang_macos_ax_host::{
    controls,
    scope::{ApplicationSpec, WindowSelector},
    scope_access::ApplicationAccess,
};

fn run(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let application = ApplicationAccess::connect(&ApplicationSpec {
        bundle_path: "/System/Applications/Stickies.app".into(),
        pid: None,
    })?;
    let windows = application.windows()?;
    println!("process {}: {windows:?}", application.pid());

    let backend = application.controls(&WindowSelector::Exact {
        title: Some("LeseLang scope initial 20261113…".into()),
        identifier: None,
        document: None,
    })?;
    let completed = controls::run(&backend, source)?;
    println!("{:?}", completed.value);
    Ok(())
}
```

For same-connection key resolution, use `WindowSelector::Key` with the exact
key from the inventory after your host has selected the intended row.
`WindowInfo` contains `key`, `title`, `identifier` and `document`; it does not
contain a transferable native pointer or an execution-bound `ObjectRef`.
The resulting `ControlsAccess` supplies those references through the existing
control catalog and session protocol.

The returned `ControlsAccess` retains its own process/window references and can
outlive `ApplicationAccess`. It continues checking its own live permission,
process and window scope. An application-inventory connection that can no
longer obtain a complete valid refresh is retired; starting a new one creates
new window keys. A separate controls connection must independently pass its
own scope checks rather than borrowing the inventory connection's last result.

An embedder that owns its event loop can use `controls::with_host` instead of
the reference runner. The flow remains explicit
`poll → authorize → dispatch → pump → accept`, followed by a fresh observation
of the intended result. Selecting a new application or window is a separate
host decision; source execution does not implicitly broaden its scope.

## Authority, Failures And Limits

The transport rechecks existing Accessibility permission, process identity,
selected window identity and the control scope before native delivery.
Window discovery and selection retain their own bounded identity registry;
control discovery keeps its existing per-control registry and observation
epochs. Reordering a window list does not redirect an existing key or selected
window.

Application admission, empty or conflicting selectors, ambiguous matches,
foreign or retired keys, process restarts, removed windows, excessive metadata
and failed native reads refuse work. Failure after entering a native action
retains the uncertain-outcome classification and does not trigger an automatic
retry. Permission revocation, cancellation, late replies, original Fuel and
previously delivered side effects keep their existing session semantics.
The selected generic window's document metadata also participates in its
observed scope epoch. A document-only change therefore fences a queued action
even if the title and control tree remain unchanged. It does not cause the
host to rerun the original window selector.

Application process discovery is bounded to 4,096 same-user process entries.
The window inventory admits at most 64 live windows and retains at most 128
window identities, including tombstones. Each key, title, identifier and
document value is limited to 4,096 UTF-8 bytes; the complete window inventory
shares a 64 KiB string budget. Exhaustion refuses the refresh rather than
discarding old identities to make room. Control inventory limits are separate.
Local document URLs are canonicalized; a bounded non-file document URL can be
reported as metadata, while document-path selection continues to accept local
filesystem documents only.

Source admission remains UTF-8 with a 16 KiB limit. Controls executions use
10,000 Fuel and return a scalar value. Native calls remain synchronous and
thread-affine. Their bounds do not turn Fuel into a preemptive native callback
sandbox. The selected window's control tree retains the limits and fixed
attribute projection documented in the [control guide](macos-ax-controls.md).

External observations cannot detect every change that returns to its old value
between reads. The application can also change after the final observation
and before the native operation. Pinning application and window identities
does not make remote read/compare/write atomic.

Run the package, native boundary and independent Consumer checks in
[CONTRIBUTING](../CONTRIBUTING.md). Generic-scope tests must cover application
path/PID admission, exact and ambiguous window matching, metadata bounds,
window reordering/replacement, changed titles, stale and cross-connection keys,
process identity and permission checks. A fake backend or pure native test
does not establish that a third application exposes compatible real controls;
desktop acceptance also requires a source run and an independent inspection
of its actual result.

## Recorded Desktop Check

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
