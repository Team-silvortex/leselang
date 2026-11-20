# Embed A Registered Application Agent Host

Two optional public helpers from `@leselang/bridge` connect an application's
controls to an agent entry. `createRegisteredApplication` builds the application
connection from control declarations. `createRegisteredAgentHost` combines that
connection with an execution transport for `AgentSession` or
`AgentWorkflowSession`, supplying plan admission, Bridge lifetime and independent
observation checks. The lower-level `RegisteredApplicationConnection`,
`AgentHost` and `Bridge` interfaces remain available.

Both helpers have no Tauri, DOM, Node or model-service dependency. Neither
compiles or evaluates LeseLang itself. The transport still owns compilation,
live grants, explicit VM driving and delivery of requests and replies.

## Declare The Application Controls

Keep the application model and its real handlers in the application. Declare
the observations and supported actions once:

```ts
import { createRegisteredApplication } from "@leselang/bridge";

let completed = false;
export const application = createRegisteredApplication({
  review: {
    read: () => ({
      role: "button", title: "Review", value: completed ? "done" : "pending",
      enabled: !completed, editable: false, clicks: completed ? 1 : 0,
    }),
    async click(context) {
      await Promise.resolve(); // Application preparation may be asynchronous.
      context.commit(() => {
        completed = true;
        context.changed("summary");
      });
    },
  },
  summary: {
    read: () => ({
      role: "text_field", title: "Summary", value: completed ? "Reviewed" : "Waiting",
      enabled: true, editable: false, clicks: 0,
    }),
  },
});

// A manual application event has its own authority and notifies the current run.
export function resetReview(): void {
  completed = false;
  application.changed("review", "summary");
}
```

The returned `RegisteredApplication` implements `attach`, `detach` and
`observe`, so it can be passed directly to the host below. Each `RegisteredControl`
has `read()` and optional `click(context)` or `setText(text, context)` handlers.
The declaration map and each control are plain objects containing those methods,
with no accessors or extra fields. The helper captures their method identities
at construction; it accepts at most 64
control names using the Bridge's 1–64 character ASCII label rules. Every
`observe()` calls the application readers again and returns owned
`RegisteredApplicationObservation` records. It does not use a cached Bridge
snapshot or the transport's report.

`RegisteredActionContext` preserves the ordinary `signal` and `commit` gate and
adds `changed(...names)`. Call this notification inside that context's
synchronous `commit` callback when other controls depend on the mutation,
including changes that return to their original visible value. It stays bound
to the action's own connection. Calling it outside the commit is rejected; a
late action cannot notify a later session. An action's own changed observations
are still detected by the Bridge.

Use `application.changed(...names)` for manual application events. It notifies
the currently attached Bridge and does nothing when no Bridge is attached;
names must still refer to declared controls. Do not substitute it for
`context.changed` inside an asynchronous agent action. The helper admits one
attachment at a time, closes an incomplete attachment on failure, and makes
cleanup of an old Bridge independent of a newer attachment.

This helper does not infer dependencies, mutate application state, schedule
handlers or roll back completed work. Its inventory is static. Applications
with conditional or remounted controls can opt into
[`createDynamicRegisteredApplication`](dynamic-controls.md), which provides
fixed logical slots, explicit `mount`, `replace` and `unmount`, and a structural
identity check through `bindingRevision`. Custom connections using lower-level
Bridge registration remain available. DOM identity checks and agreement between
the model and rendered controls still belong in application readers and handlers.

To describe tasks over those controls, use the optional
[`defineRegisteredTask` constructor](registered-tasks.md), then validate the
expanded declarations against the real base catalog with
[`defineSemanticCatalog`](semantic-authoring.md). Control registration describes
available objects; the task declaration separately states the intended workflow
and the evidence required for success.

## Connect The Application And Transport

This composition function accepts the application above or any custom
connection implementing the same public interface. The execution transport is
supplied separately; the function does not hide a Rust runner or network service:

```ts
import {
  AgentSession,
  createRegisteredAgentHost,
  createSemanticProvider,
  type RegisteredApplicationConnection,
  type RegisteredAgentTransport,
} from "@leselang/bridge";

export function connectAssistant(
  application: RegisteredApplicationConnection,
  transport: RegisteredAgentTransport,
  freshEpoch: string,
) {
  const host = createRegisteredAgentHost({
    window: "my_app",
    epoch: freshEpoch,
    application,
    transport,
    observations: { before: "native_before", after: "native_after" },
  });
  const session = new AgentSession({
    provider: createSemanticProvider(),
    host,
  });
  return {
    submit: (message: string) => session.submit(message),
    cancel: () => session.cancel(),
    close() {
      session.cancel();
      host.close();
    },
  };
}
```

The offline provider accepts commands derived from the actual catalog, such as
`task read_review_status`. It needs no model credits. A model provider or a
bounded sequential workflow can use the same host; see
[application agent entry](agent-entry.md) and
[sequential workflows](agent-workflows.md).

`window` is the historical protocol name for an application endpoint, including
an endpoint with no desktop window. Both `window` and `epoch` use the Bridge's
1–64 character ASCII label rules. Give each application lifetime a fresh epoch
and let exactly one host instance own that endpoint. The helper generates
monotonically increasing execution sessions within it; it never reuses a
session or depends on a global random-number API. Recreating a host requires a
new epoch, even if the application window has the same name.

## Optional Execution Failure Reports

Set `executionReports: "v1"` explicitly when the transport returns the bounded
terminal failure report contract. The registered host exposes this choice to
`AgentWorkflowSession`, so its referenced-source verification uses the same
rules. Failure reports retain original evidence and stop before success-budget
checks; even complete successful fields cannot override a terminal failure.
Existing connections without the option keep generic `status` business fields
uninterpreted. See [execution failure reports](execution-reports.md) for exact
shapes, typed Rust producers and cancellation priority.

## Custom Application Connections

`RegisteredApplicationConnection` has three required synchronous methods and an
optional structural identity reader:

| Method | Application responsibility |
| --- | --- |
| `attach(bridge)` | Register the current application objects and connect their real handlers to this execution's Bridge. |
| `detach(bridge)` | Remove that execution's connection without detaching a newer connection. It must tolerate cleanup after an incomplete attachment. |
| `observe()` | Return a fresh, side-effect-free inventory of the application's actual public control state. |
| `bindingRevision?()` | Return a nonnegative safe integer that increases on every structural binding change, including same-value replacement and ABA. |

Each observation contains at most 64 records with exactly these fields:

```ts
{
  identifier: "review",
  role: "button",
  title: "核对示例报告",
  value: "pending",
  enabled: true,
  editable: false,
  clicks: 0,
}
```

Identifiers must be nonempty and unique. Text fields are bounded UTF-8 strings;
`clicks` is a nonnegative `u32`. The host owns and validates the data without
invoking getters, custom serialization or coercion, and compares inventories
by identifier rather than array position. These application observations do not
include the protocol's execution-local `generation` and `revision` fields.

The optional binding revision is separate from observations and wire data.
The host checks it at execution boundaries and before/after independent
verification. A changed identity stops execution or yields
`application_bindings_changed` verification detail even when every visible value
is unchanged. Already dispatched effects may remain; these checks do not provide
atomic revocation. Connections without this hook retain their existing behavior.
See [dynamic controls](dynamic-controls.md) for lifecycle and failure contracts.

Observe the application's model and actual controls, not a cached report returned
by the transport. For a DOM application, also check that the registered nodes
are still the intended instances. See the
[task board model and view](taskboard-host.md#application-boundaries) for a
reference that independently checks its model and DOM.

Custom adapters implement `read`, `click` and `setText` through `Bridge`.
Asynchronous handlers must enter `context.commit` before each mutation after an
await. The helper closes the Bridge itself when work is cancelled or retired;
that protection does not depend on `detach` remembering to close it. Existing
mutations remain when later work fails. The
[JavaScript object contract](javascript-sdk.md#register-an-application-adapter) describes
registration, replacement and mutation gates.

## Execution Transport

`RegisteredAgentTransport` is implemented by trusted application code:

| Method | Contract |
| --- | --- |
| `describe(signal)` | Return the application's current semantic catalog; preserve original JSON text for exact integers. |
| `execute(run, signal)` | Compile and execute the admitted source and arguments, route host requests to `run.onRequest`, and resolve with the complete original report JSON text. |
| `reply(reply)` | Deliver the Bridge reply to the same correlated execution. Delivery failure must reject or throw. |
| `cancel(scope)` | Request cancellation of that exact execution and arrange for its pending execution Promise to settle. |
| `close(endpoint)` | Retire the endpoint's transport resources and settle pending work. |

`run` is a `RegisteredAgentRun` containing a readonly `scope`, `source`,
`arguments` and `onRequest(request)` callback. The scope carries the exact
endpoint and execution identity. Source and literal arguments remain separate;
the transport must not interpolate arguments into source. Calls to `onRequest`
are handled by that run's Bridge, and replies are delivered through `reply`.

The transport must enforce its own bounds, deadlines, message correlation and
authentic endpoint routing. An aborted signal or a requested cancellation is
not evidence that a remote action stopped. Keep the execution Promise pending
until the transport has actually settled its work, and report uncertain or
partial outcomes accurately. This helper neither retries delivery nor creates
a background scheduler. The Rust VM continues its explicit
`poll → authorize → dispatch → accept` sequence, with any host pump remaining
explicit.

The Tauri adapter maps these callbacks to its existing `Channel` and `invoke`
commands. Other applications can supply their own carrier without importing
Tauri. This is an embedding interface for trusted adapters, not a network
protocol or a sandbox for arbitrary JavaScript callbacks.

## Admission And Evidence

`describe` admits the application's catalog independently of the provider.
`execute` rebuilds the selected semantic task and requires its source and
arguments to match the proposal before starting transport execution. A summary
or a provider's verification claim cannot replace this contract.

The trusted `observations` option names two top-level report fields. Use
`native_before` / `native_after` for the Tauri carrier, or
`objects_before` / `objects_after` for a carrier with that report layout. These
names belong to host configuration; neither the provider nor the program may
choose them. They must be distinct valid top-level field names. The fields must
hold the registered-control observations described above.

Acceptance requires all of the following:

1. The original report passes the admitted task's semantic verifier, including
   profile, result, Effect/Fuel bounds and declared predicates.
2. The plan and raw report match this execution's retained evidence exactly.
3. The report's before-state equals the independently captured pre-execution
   application state.
4. The report's after-state equals a fresh application observation at
   verification time, while the execution is still eligible for acceptance.

Evidence is consumed once. Cancellation, closing, a mismatched report or plan,
or a change in current application state prevents acceptance. Original report
JSON preserves exact `u64` values; ordinary JavaScript parsing is used only for
the independently bounded control observations, after semantic verification.
An already parsed report object is not accepted by this helper. Custom
`AgentHost` implementations remain available for other evidence models.

These checks establish the declared postcondition at observation time. They do
not make observation and mutation atomic, stop changes after verification, or
prove that a model understood the user's intent.

## Lifetime And Verification Scope

The host admits one description or execution at a time. Cancellation immediately
closes the local mutation gate but retains the occupied execution slot until
the transport Promise settles. Application callbacks cannot reenter the host
to start overlapping work, and closing or cancellation during a callback is
checked again before transport execution. Late requests and reply failures
remain bound to their original run, so they cannot close a later run.

The transport receives an execution-local `AbortSignal`. Closing the host or
aborting the caller's signal also aborts that signal during a pending
description; its slot likewise stays occupied until the description Promise
settles. Optional `onSource(source)` can display the canonical program before
execution, but cannot replace it or change the application's contract.

The packed-package Consumer exercises this public helper with an independent
in-memory application and transport, including TypeScript declarations. It
establishes package independence from Tauri; it is not a Rust VM test. The
original independent Node Consumer continues to exercise its Rust-launched
application. The additional [Node-owned agent](node-agent-host.md) now uses this
public helper and `@leselang/bridge/node` with a real per-task Rust worker and
keeps the application model alive across tasks. Its separate packed Consumer
repeats execution, failure and cancellation from an external tarball installation.
The Tauri form and task board use the public helper through the Tauri
transport, with actual desktop verification recorded separately in the
[registered-host acceptance record](acceptance/agents.md#public-registered-application-host).

Run the `javascript`, `node-consumer`, `tauri-portable` and `tauri-desktop` check
groups as applicable. See [testing requirements](testing.md) for the complete
scope and platform requirements.
