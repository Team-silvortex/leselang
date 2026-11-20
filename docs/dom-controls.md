# Bind Application DOM Controls

The optional `@leselang/bridge/dom` entry connects application-owned text fields
and buttons to `createRegisteredApplication`. Manual events and Lese operations
use the same declared business handlers. The helpers own event correlation,
completion/error delivery and binding lifetime; the application still owns its
model, observations, semantic catalog and verification conditions.

This is an opt-in adapter for applications that can register their own handlers.
It does not discover arbitrary websites, evaluate agent JavaScript, synthesize
trusted user input or manage framework state. Importing the entry does not access
the DOM, so an application can import it during server rendering and create its
bindings only after mounting. The package has no additional runtime dependency.

## Declare A Binding

`createDomTextControl` and `createDomButtonControl` return a frozen object with
`control` and `dispose()`. Pass the control to the existing registered application:

```ts
import { createRegisteredApplication } from "@leselang/bridge";
import { createDomTextControl, createDomButtonControl } from "@leselang/bridge/dom";

// These are the application's mounted elements and independent state readers.
// Use <button type="button">; form submission is a separate application action.
const draftBinding = createDomTextControl({
  element: draftElement,
  read: readDraftState,
  changed: (...names) => application.changed(...names),
  validateText(text) { validateDraft(text); },
  onInput(text, context) {
    // Already inside one synchronous commit with the DOM value assignment.
    model.updateDraft(text);
    context.changed("draft", "save");
    renderDraftStatus();
  },
  onError(error, origin) { showInputFailure(error, origin); },
});

const saveBinding = createDomButtonControl({
  element: saveElement,
  read: readSavedState,
  changed: (...names) => application.changed(...names),
  beforeCommit() {
    // Check related controls and model/DOM agreement again after any await.
    readDraftState();
    readSavedState();
  },
  async onClick(context) {
    const prepared = await model.prepareSave(context.signal);
    context.commit(() => {
      model.commitSave(prepared);
      renderSavedState();
      context.changed("save");
    });
  },
  onError(error, origin) { showSaveFailure(error, origin); },
});

const application = createRegisteredApplication({
  draft: draftBinding.control,
  save: saveBinding.control,
});
```

The `changed` callback serves manual interactions. Agent interactions instead
retain the `RegisteredActionContext` supplied by their particular bridge action;
they cannot borrow a later session's authority. Keep change notifications inside
the commit that changed the model. A control whose observation depends on another
field must also be notified, including edits that later return to the old value.

`read()` remains application code. It should compare actual DOM values, labels
and rendered results with independent domain state and return a `ControlState`.
Returning a cached model alone does not establish that the GUI reflects it.
The helper's pinned-element checks complement these application checks; they do
not replace them. The current form, task board and article views illustrate
different application readers and business handlers using the same bindings.

Bindings require a mounted element with a nonempty, stable ID in its own
document. Text controls are `textarea` or text-like `input` elements (text,
search, URL, telephone, email or password); button controls require
`type="button"`. Other widget families remain available through custom
`RegisteredControl` handlers. Binding the same element twice is rejected;
dispose the old binding before binding that instance again.

## Text And Button Completion

Text assignment and `onInput` run in one synchronous commit. `validateText` runs
before an agent changes the DOM value. The input handler receives only `signal`
and `changed`, because nesting another commit would be invalid. It must finish
synchronously; asynchronous application work belongs in an explicitly declared
button action or a custom lower-level control. Manual input has already changed
the browser field when its event arrives. If the application rejects that input,
its manual error handler may restore its last valid draft, as the Article view
does. The binding does not automatically roll back application effects.

Button handlers return `undefined` or a current-realm `Promise<void>`; arbitrary
thenables and foreign-realm Promises are not supported. Text `onInput` must
return `undefined` synchronously. Every domain mutation after awaiting must
reenter the supplied `context.commit`. `beforeCommit`, when supplied, runs inside
that gate and can recheck other controls or application invariants. Handler
completion belongs to that invocation, even if another manual interaction occurs
while it is pending. Errors from declared handlers and synchronous hooks,
including `throw undefined`, are
captured for the originating operation rather than relying on `dispatchEvent`
to rethrow a DOM listener exception.

`onError(error, origin)` is an optional application presentation hook, where
`origin` is `"agent"` or `"manual"`. It cannot turn a failed operation into a
successful one. Do not use it to retry, publish a result or bypass a commit gate.
An error after a mutation retains that mutation, following the bridge's existing
partial-effect rules.

Validation and `beforeCommit` hooks are synchronous checks. Do not start
asynchronous work in them: an asynchronous result is refused before the domain
mutation continues. Button work that needs to await must return its Promise
from `onClick`, then perform its mutation through `context.commit`.

## Events And Lifetime

Agent operations dispatch a fresh ordinary `Event` to the pinned element.
Only that exact event can consume its private action association, once. The
event does not carry a context or authority token in `detail` or public fields.
Extra synchronous events cannot reuse the agent context. Independent manual
events use application-owned authority and cannot overwrite the completion of
an outstanding agent operation.

These events are synthetic: they are not trusted keyboard or pointer gestures,
do not focus elements, and do not promise the default behavior of a native
`.click()`. The button adapter targets explicit `type="button"` handlers.
Register an action handler once; remove the old duplicate listener when adopting
a binding. Framework integrations must update state through the framework's own
business handler and verify the resulting render in their application reader.

An element's identity remains pinned to its binding. Replacement or detachment
requires disposal and a fresh binding; the helper does not silently follow a
selector to a new element. Dispose each binding when its view unmounts:

```ts
host.close();
draftBinding.dispose();
saveBinding.dispose();
```

Disposal removes the helper's listener and fences its retained callbacks. It is
idempotent. A captured context also expires when its handler returns synchronously
or its Promise settles; it cannot be saved as authority for a later commit.
Session detach is separate: ordinary manual interactions can continue
after a session ends, while callbacks from that old session cannot commit.
Closing the host and disposing the view are both application lifecycle duties.

For conditional views, use the optional
[`createDynamicRegisteredApplication`](dynamic-controls.md). It keeps fixed
logical names while allowing explicit mount, replacement and unmount of their
controls. Unmount the slot before disposing and removing its element; create
and bind a new mounted element before mounting that slot again. The registered
host checks structural identity separately from visible values, so a report
from the old instance cannot verify against a same-value replacement.

The helpers add no scheduler, automatic retry or rollback. Catalog generation,
the Rust VM, Fuel, live authorization and the explicit
`poll → authorize → dispatch → accept` loop remain in their existing layers.
See [registered application hosts](registered-agent-host.md),
[semantic task authoring](semantic-authoring.md), and
[verification diagnostics](verification-diagnostics.md) for those contracts.
