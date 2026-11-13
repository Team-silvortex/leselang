# Local Host Protocol Draft, Version 1

LeseLang is an agent control language for explicitly adapted hosts. The first
target is GUI control: discover available operations, observe objects, submit
authorized actions against the state that was observed, and verify what changed.
CLI hosts use the same contracts. A host can expose a control panel, an editor,
a command session or another resource without changing the evaluator.

This document describes the local Rust protocol in
`leselang_vm::native::core::host_protocol`. Version 1 is an exact local contract
identifier. It does not declare a stable wire format, FFI ABI, registry release
or compatibility with another product's protocol. No transport, persistence
layer, wait scheduler or GUI framework is installed by this protocol.

## Discover Before Acting

The host's `OperationCatalog` remains the authority for operation names,
parameter domains, result declarations, required capabilities and catalog
version. The scalar embedding adapter's `Host::describe()` exposes immutable
discovery metadata for its registered operations without running callbacks.
An agent can use this metadata to construct source appropriate to that host.

`HostDescription::protocol_version()` identifies this local protocol;
`operations()` lists `OperationDescription` values exposing `name()`,
`parameters()`, `result_type()` and `required_capability()`. These descriptions
borrow the built host's immutable registration metadata. Native adapters with
non-scalar result domains use the existing `OperationCatalog::schemas()` to
inspect the original schemas in declaration order. Discovery introduces no
parallel registry or conversion of native result domains into scalar types.

Discovery describes what a host supports; it does not grant permission to call
it or attest that a particular resource is currently available. Compilation
checks the chosen catalog and declared grants. The host must still check live
capabilities, object identity and state immediately before a native action.
Operation catalog versions and this protocol version are separate contracts.

| Identifier | What it identifies |
| --- | --- |
| Protocol version | This local Rust host contract. `ProtocolVersion::from_number` accepts exactly V1; `number()` exposes its numeric value. |
| Operation catalog version | The host's operation-schema lineage, checked by existing catalog lookup and authorization. |
| Object generation | The current native identity occupying an owner's object slot; replacement invalidates older references. |
| Object revision | State or admitted-action changes within a generation; used to reject work based on stale observations. |
| VM request serial and execution identity | Correlation of one request and reply within a particular session. |

Equal numeric values in these domains do not make them interchangeable. V1
acceptance is an exact local check, not negotiated wire compatibility.

## Object Identity And Lifetime

`LocalObjectOwner` owns one local identity. It creates opaque, weak `ObjectRef`
references tied to both an object generation and a VM execution lease. A
reference cannot be created from an integer or serialized through this API.
It does not keep the object owner or execution scope alive.

Before access, validation checks all of the following:

- The reference belongs to this owner.
- Its generation is still current.
- The bound execution lease is open and matches the supplied execution scope.
- The owner has not been retired or dropped.

`replace` advances the generation and invalidates references to the previous
object before native replacement. Call `retire` before native teardown; retirement
and owner drop permanently invalidate its references. Closing the VM scope
invalidates references bound to that execution, even when a host
still retains the underlying native object. Hosts keep thread-affine GUI
objects and these local references on their owning thread.

An object reference is an identity and lifetime constraint, not a capability
grant. Passing validation does not bypass the operation catalog or current
host policy. Native resource access must remain behind the same host boundary.

## Observe, Compare And Attempt

`observe` produces an `Observation<T>` containing the object reference, its
revision and already captured snapshot data. The adapter captures the data from
the same native object, validates its schema and bounds, and prevents an
intervening change before stamping it with `observe`; the helper does not read
the resource itself. A snapshot records what the host saw at that
revision. Retaining or reading the snapshot later does not imply that its
object remains live, its data remains current or its reader remains authorized.
Adapters supply stable snapshot data and avoid embedding mutable native access
inside `T`.

An action uses `with_current` to compare its expected revision with the live
revision after identity and lease checks. If they differ, the host must reject
the stale attempt without entering its callback. The caller should obtain a
new observation and decide what to do with it; a stale result is not an
automatic retry instruction.

On admission, `with_current` advances the revision **before** invoking native
code. The old revision is invalidated even if the callback returns an error or
panics. This prevents the same observation from being silently reused after an
attempt that may have changed the resource. `ActionAttempt` returns the native
output and revision context; it makes no claim that the action was submitted,
completed or successful. Reentrant native code may already have superseded the
admitted revision by the time the callback returns.

Use `advance_revision` to fence changes to relevant resource state.
For a GUI host, this includes changes made by the user, another controller or
the framework's event processing. Advance before host-controlled mutation and
before publishing an externally refreshed state. Revision checks work only if
the adapter records the changes that matter to its operations. A revision is
concurrency data, not an object handle, credential or proof that an external
effect occurred. An external event may already have happened before the adapter
can record it. If tracking fails then, retire the owner and refuse further
actions; retaining the old revision would misrepresent the live state.
Generation and revision counters do not wrap. If an increment fails, the adapter
must not perform the corresponding replacement or mutation.

## Submission Is Different From Observed State

A minimal control cycle is:

1. Observe the object and retain its revision and relevant properties.
2. Submit an action with that expected revision under current permissions.
3. Interpret the host's action response according to its declared result type.
4. Observe again after the host has processed the action, then test the intended
   postcondition against the new state.

A GUI dispatcher may acknowledge that an action entered its event queue while
the widget still has its old value. That acknowledgement records submission.
It does not prove that the widget changed, that rendering completed, or that a
later observation will satisfy the requested postcondition. A CLI adapter may
complete an in-memory mutation synchronously, but it should make that stronger
claim explicit in its result schema rather than assuming every host behaves
that way.

Queued work must retain the bound object reference, execution lease and its
admitted revision. The event pump rechecks those fences and current permissions
at the actual mutation boundary. An earlier authorization or queue acceptance
does not permit work after cancellation, replacement, revocation or a conflicting
state change.

The reference hosts exercise `find → observe → apply(expected_revision, text) →
observe` with the same conceptual contract. Both headless models use explicit
queues and change observed state only when the host pumps an event. The pump
rechecks object identity, execution lease, revision and current grants before
mutation. Submission acknowledgements and observed postconditions are separate.
Their native state and snapshot representations differ; they share the contract
without sharing a product DTO. Both use real LeseLang source, typed native
result fields and the existing VM handoff protocol.

The program compiled by both adapters is:

```text
fn main() = bind(
    target: object.find(name: "save"),
    body: bind(
        before: object.observe(target: target),
        body: bind(
            submitted: object.apply(
                target: target,
                expected_revision: field(value: before, name: "revision"),
                text: "ready",
            ),
            body: bind(
                after: object.observe(target: target),
                body: field(value: after, name: "text"),
            ),
        ),
    ),
)
```

These operations are supplied by the reference adapters. `object.find` resolves
the host's name and returns a typed native object result. The source retains
that result in `target` and passes it to subsequent operations as an object,
without converting it to an integer or string handle. The reference host exposes
both `save` and `other`, so routing follows the returned object identity rather
than an operation key fixed to one object. Names are lookup input; they are not
the object references themselves.

The scalar CLI's `std.print` catalog does not contain these operations. The
reference driver pumps queued work inside the original VM's metered host-work
boundary while its submission reply is still pending, then explicitly accepts
that reply. A subsequent observation establishes the changed state.

Each reference host owns one session with at most one outstanding request. Its
result admission tracks the exact object discovered or requested, so a reply
for `other` cannot satisfy an observation of `save` even within the same scope.
This pending-request state survives ordinary reply rejection and is checked
again on re-entry. An adapter supporting multiple outstanding requests must
keep equivalent validation state per request; a shared latest-target slot would
not provide that contract. The
[conformance tests](../crates/leselang-vm/tests/host_protocol.rs) cover dynamic
routing, aliases, cold type errors and wrong-object replies.

Run the [reference hosts](../crates/leselang-vm/examples/protocol_hosts.rs) from
either workspace:

```sh
cargo run --example protocol_hosts --locked --offline --profile lean
cargo run --manifest-path consumer/Cargo.toml --example protocol_hosts --locked --offline --profile lean
```

The separate [AppKit host](appkit-host.md) uses the same typed object flow with
real macOS `NSButton` and `NSTextField` controls. Its `gui.list` returns bounded
identifier metadata, while `gui.set_text` and `gui.click` queue a value replacement
or target/action invocation. The driver explicitly pumps each action and then
requests a new observation to verify the edited value or Save's captured value
and click count. Input edits also invalidate Save's earlier observation, so the
form's dependency cannot silently admit a stale submission. Framework objects,
event delivery, identity tracking and the Objective-C bridge stay in the host
workspace.

The separate [macOS Accessibility host](macos-ax-host.md) scopes the same native
flow to one TextEdit document or a Calculator Basic-mode window in another
process. Both profiles use the same typed session driver. TextEdit finds the
configured document's text area, observes it and queues a complete text
replacement. Calculator finds a window capability, observes the display and
allowed buttons, and queues `gui.press` with a named button. Those names are
metadata resolved under the live window capability, not serialized object
references. Process start identity, the selected document or window and retained
AX element equality are checked again before delivery. Calculator also compares
the display, optional last expression and all allowed button states, since the
meaning of a press depends on the current calculation. A changed expression can
invalidate a queued press even when the displayed result stays the same.
Subsequent observations verify the remote values.

The opt-in [controls mode](macos-ax-controls.md) extends discovery within those
same explicit process/window scopes. It lists connection-local keys or resolves
exact role/identifier/label selectors into distinct per-control native
references. Empty and ambiguous selectors fail; registry slots and retired
keys are never reused. Discovery metadata is not serialized authority and a
new connection cannot reuse an earlier key. Observe and action replies remain
bound to the requested control's owner and execution lease. Presses and text
replacements use the shared explicit queue and compare the full observed scope
before native delivery, including other controls and non-target dependencies.
An unchanged button cannot admit a stale action after an observed calculation
context change.

The [application/window selection layer](macos-ax-applications.md) extends the
host's admission step to an exact running macOS bundle and optional PID. It
lists bounded window metadata, requires a unique exact metadata match or a
same-connection window key, and retains the resulting native window identity.
A title change does not switch targets; disappearance or replacement refuses
the selected connection. Process executable/start checks and window registry
tombstones preserve that boundary. The resulting controls connection uses the
same catalog and execution-bound object references; source cannot broaden its
application or window scope through an Effect.

The optional [widgets profile](macos-ax-widgets.md) uses the same boundaries for
popup menus, explicit checkbox state, native item selection and scrollbar
position. Its item arguments are scoped native references. The host validates
current ownership and membership before delivery, including transient menu
items whose acknowledgement must remain associated with a stable menu owner.
Relevant checked, selected and scroll state are observation dependencies;
operation support alone is not evidence that a requested postcondition holds.
Its exact role/value selector is a read operation that returns a native
reference under the existing execution lease. Zero or multiple matches fail,
and finding a displayed value grants no additional write capability.
An [auxiliary-process grant](macos-ax-auxiliary-processes.md) can explicitly
admit helper-owned widget objects reachable within the same pinned window.
The host approves fixed PID/start-time/executable identities and can revoke a
shared grant. Discovery supplies metadata only; it does not authorize a helper
or add that process's other windows. Process and grant checks also run when
accepting targetless discovery replies. This is native transport policy and
introduces no source-language operation.

This external adapter advances revisions for observed changes and admitted
attempts. It cannot observe all intervening remote mutations: an A→B→A change
entirely between reads may be missed, and either application can change after
the final check but before the native mutation. The revision therefore does not establish atomic
remote compare-and-set. The application-owned AppKit host retains its stronger
mutation tracking. Native object identity, execution leases, original Fuel,
permission checks and reply correlation remain required for both adapters.

There is no implicit `wait_until`, timer or polling service. A host that needs
to wait owns its event loop, deadlines, cancellation delivery and bounded
observation policy. A future wait contract needs explicit evidence for those
semantics before it becomes part of this protocol.

## Keep The Effect Boundary Explicit

Every Effect still follows `poll → authorize → dispatch → accept`:

| Boundary | Responsibility |
| --- | --- |
| `poll` | Evaluate a language slice and yield a typed request. |
| `authorize` | Check current execution authority and host capabilities. |
| `dispatch` | Recheck live authority, validate the object and expected revision, then attempt native work. |
| `accept` | Validate the corresponding reply before a later explicit poll resumes the language. |

The local object helpers add identity and revision checks to the host's action
boundary. They do not replace `LocalVm`, authenticate external messages, spend
Fuel automatically, or turn an `ActionAttempt` into a VM receipt. The host
continues to bound native work and use the original session meter.

Keep rejection, execution outcome and observed postcondition distinct:

| Situation | Meaning and handling |
| --- | --- |
| Wrong owner/generation/scope, revoked permission or stale revision | Refuse before the native action. The host chooses whether to expose a typed domain refusal or terminate the request. No retry is implied. |
| Submitted or completed native work awaiting reply acceptance | Use the existing pending host-work path; the declared receipt states what the host actually knows. |
| Known failure after native work begins | Classify through `HostWorkOutcome::Failed` when terminating the VM. Earlier or partial side effects remain. |
| Delivery or completion cannot be determined | Use `HostWorkOutcome::Uncertain`; reconcile with later observations or host-specific recovery. |
| Cancellation | Use the existing cancellation path and deliver cancellation to external work explicitly. Previously committed effects remain. |
| Reply is late, mismatched or invalid | Existing receipt validation rejects it while preserving the original ownership rules. It does not authorize replay. |

The existing `HostWorkOutcome` and VM statuses remain the execution contract;
this protocol adds no outcome scheduler. A callback panic still follows the
existing `LocalVm` uncertainty fence. Revision advancement is conservative
invalidation, not rollback or proof of an action's result.

## Native Object Operands

Native HIR hosts can opt into operations taking typed native result operands.
The source compiler retains `PureType::Result` metadata through ordinary `Call`
arguments, including variables established by `bind`. It uses the existing
parser, control-flow compiler, structural limits and helper hygiene. Object
operands are visible HIR children; no source rewriting or opaque local-name
payload is needed.

`CallFlowEnvironment::call_native_argument_type(operation, parameter_name,
result)` is the static admission hook. Its default is `false`, so existing
catalog adapters remain scalar-only unless they explicitly declare which native
result types an operation parameter accepts. This metadata check runs during
compilation, including cold branches. It cannot validate an actual object's
current generation, execution lease, revision or permissions.
Scalar operands still use their original catalog domains. For a parameter that
accepts only native objects, use a scalar domain that rejects every scalar, then
admit the exact native result type through the hook. This prevents a number or
string from slipping into an object parameter as an alternative representation.

At runtime, native adapters use `PreparedValueCall` and
`evaluate_value_call_arguments_in_scope` for mixed scalar/native arguments.
This evaluates operands once in declaration order after physical/scope/signature
preflight, then invokes a mandatory validation callback for every supplied
operand. The callback checks each scalar domain or native type, owner, lease and
value bounds; no domain is implicitly accepted. It receives the original Fuel
for additional native accounting. Native local values follow the existing clone
contract, so use bounded opaque references or shared snapshot views.

The preparation function does not select or authorize a catalog version. The
adapter selects the original schema and rechecks current policy before dispatch;
passing a static type check is not object authority. `PreparedValueCall` retains
that borrowed schema and move-only prepared operands. Scalar-only adapters keep
their existing `PreparedCall` path. Both use the original evaluator and Fuel,
with explicit request correlation, dispatch and reply acceptance. Native
validation errors stay external failures and do not become recoverable scalar
errors.

Helper input annotations remain the six scalar types. A helper may return a
native object result, which its caller can bind and pass to a host operation;
an `object` helper parameter annotation is not yet supported. Snapshot `field`
projections also remain scalar, so retain the discovered object separately
from its observation's revision and text.

The `leselang_vm::embedding` convenience API and standalone CLI still expose
their scalar registration profile. Native object operands are an opt-in native
HIR facility, not a new scalar value, universal GUI toolkit binding or wire
handle. A scalar revision or name never substitutes for the returned object
identity.

## Evidence And Remaining Gates

| Milestone | Current evidence and boundary |
| --- | --- |
| Reusable Rust embedding | Registration, compilation and explicit session driving use the existing HIR/VM; independent consumers test Fuel, revocation, cancellation, late replies and partial effects. |
| Standalone scalar CLI | `check` and `run` execute complete source programs; `std.print` is an explicitly granted reference host operation. |
| Local protocol draft | Opaque object identity, generation, execution lease, revision checks and historical observations are exercised through GUI/CLI headless host conformance tests with distinct native representations and explicit queues. |
| Dynamic native object operands | Real source discovers and binds an object, then passes the typed result to observe/apply operations; two objects exercise identity-based routing. Scalar adapters reject native operands by default. |
| Real GUI adapter | The separate macOS AppKit host binds actual `NSButton` and `NSTextField` objects to bounded discovery, text replacement and target/action dispatch through the existing VM protocol. Its form verifies edited and submitted values and tracks Save's dependency on the input revision. Its desktop self-test exercises the full source flow and live object/session fences; macOS CI builds, lints and runs the non-GUI package tests separately. This establishes an application-owned AppKit adapter, not cross-platform or other-process automation. |
| Scoped cross-process GUI adapter | The separate macOS Accessibility host has completed a four-Effect text replacement flow against a real TextEdit document and a ten-Effect `1 + 2 = 3` flow against Calculator 12.0 on macOS 27.0.1, each with separate window inspection. Calculator uses a window capability, observed display/last-expression/button dependencies and queued `AXPress` actions through the same typed driver. Both check live permission, process identity, scoped targets and AX element identity. External freshness is snapshot based, with a remaining read/action race and no guarantee against unseen ABA changes. Desktop acceptance requires the supported application state and host Accessibility permission. |
| Scoped control discovery | The opt-in controls catalog supports bounded inventories, exact unique selectors, separate native references for buttons/text controls and explicit press/text operations under the existing TextEdit or Calculator window scope. Real controls-mode runs completed the 17-Effect `8 + 7 = 15` Calculator source and four-Effect TextEdit replacement, each with separate desktop verification. TextEdit also completed a three-Effect list/find/observe source; real ambiguous selectors and keys from earlier connections refused. Whole-scope observed changes fence queued actions under the declared attribute projection; this does not expand application scope or provide atomic remote state. See the control guide for the exact tested layouts and regression checks. |
| Generic application/window selection | An explicit bundle path and optional matching PID admit one running same-user application instance. Bounded window metadata and exact unique selection or same-connection keys bind one native window independently of later title changes. The generic controls path completed a six-Effect two-replacement flow against a dedicated Stickies 10.3 note, retaining the selected window through changing titles and independently verifying its final text. Window/process replacement, permission loss and observed state changes still refuse stale work; this is macOS scope selection with per-application compatibility requirements, not atomic remote state or universal GUI coverage. See the application selection guide for the tested flow and boundaries. |
| Common widget operations | The optional 13-operation widgets catalog adds exact role/value discovery, scoped menu operations, desired checkbox state, native item selection and normalized scrollbar position. A real Finder workflow used explicit View Options and folder-window sessions, opened/cancelled/chose a popup item, cleared Size/Kind checkboxes, selected one exact native row and scrolled from 0 to 1,000, with independent desktop inspection. The menu choice's immediate observation had not updated, so its source stopped; a later explicit read established the postcondition before the next session continued. The [widget guide](macos-ax-widgets.md) records each source result and the same-process boundary, including refusal of Preview's XPC Export panel. Portable and native tests separately preserve identity, failure, Fuel and session fences. |
| Explicit auxiliary process scopes | A reviewed PID/start-time/executable grant admits helper-owned objects only through the selected window's bounded AX relationships, with shared monotonic revocation and repeated process checks. Preview's Export panel completed a six-Effect source flow that wrote and observed two successive export-name values in an AppKit helper, followed by independent desktop inspection and cancellation without exporting. Default ungranted access and a mismatched start time refused. The [auxiliary-process guide](macos-ax-auxiliary-processes.md) records the tested instance, embedding API and boundaries; the helper's other windows remain outside scope. |
| Wider object language and remote/durable hosts | Native helper parameter annotations and object-valued field projections remain separate language extensions. Remote/durable hosts still require transport identity, version negotiation, failure recovery and compatibility decisions; local references cannot be reused as wire handles. |

See the [embedding guide](embedding.md) for Rust host setup and the
[architecture](architecture.md) for ownership and dependency boundaries.
