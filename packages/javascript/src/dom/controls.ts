import type { ControlState } from "../bridge.js";
import type { RegisteredActionContext, RegisteredControl } from "../agent/registered-application.js";

export type DomEventOrigin = "agent" | "manual";
/** Input callbacks are synchronous and already inside the action's commit. */
export interface DomInputContext {
  readonly signal: AbortSignal;
  changed(...names: string[]): void;
}
export interface DomControlBinding {
  readonly control: RegisteredControl;
  /** Remove this binding and fence continuations captured before disposal. */
  dispose(): void;
}
interface DomOptions {
  read(): ControlState;
  /** Forward manual changes to the owning RegisteredApplication. */
  changed(...names: string[]): void;
  /** Notification only: throwing cannot replace or recover an action result. */
  onError?(error: unknown, origin: DomEventOrigin): void;
}
export interface DomButtonControlOptions extends DomOptions {
  readonly element: HTMLButtonElement;
  onClick(context: RegisteredActionContext): void | Promise<void>;
  /** Recheck application-owned dependencies inside every explicit commit. */
  beforeCommit?(): void;
}
export interface DomTextControlOptions extends DomOptions {
  readonly element: HTMLInputElement | HTMLTextAreaElement;
  /** Runs before an agent assigns DOM.value, and before every input callback. */
  validateText?(text: string): void;
  onInput(text: string, context: DomInputContext): void;
}

type Element = HTMLButtonElement | HTMLInputElement | HTMLTextAreaElement;
interface Invocation {
  readonly event: Event;
  readonly origin: DomEventOrigin;
  context: RegisteredActionContext;
  consumed: boolean;
  failed: boolean;
  error?: unknown;
  notified: boolean;
  finished: boolean;
  text?: string;
  completion?: Promise<void>;
  check(): void;
  actionable(): void;
}
function fail(code: string): never { throw new Error(code); }
function requireFunction(value: unknown): void {
  if (typeof value !== "function") fail("invalid_dom_control_options");
}
const bindings = new WeakMap<EventTarget, Set<string>>();
// This guard lasts only for synchronous agent event dispatch, never an await.
let issuing: { readonly event: Event; readonly owner: object; reject(error: unknown): void } | undefined;
function asyncResult(value: unknown): boolean {
  if (value instanceof Promise) {
    // Refusing an async input/commit must not create an unhandled rejection.
    void Promise.prototype.then.call(value, undefined, () => {});
    return true;
  }
  return (typeof value === "object" && value !== null || typeof value === "function") && "then" in value;
}

/** Event identity, not ambient context, associates an agent call with its handler. */
function bind(element: Element, options: DomOptions, kind: "button" | "text",
  handler: (invocation: Invocation) => void | Promise<void>, beforeCommit?: () => void) {
  requireFunction(options.read);
  requireFunction(options.changed);
  if (options.onError !== undefined) requireFunction(options.onError);
  if (beforeCommit !== undefined) requireFunction(beforeCommit);
  const read = options.read.bind(options), changed = options.changed.bind(options), onError = options.onError?.bind(options);
  const document = element?.ownerDocument, id = element?.id, tag = element?.tagName;
  const inputType = tag === "INPUT" ? (element as HTMLInputElement).type : undefined;
  if (!document || typeof document.getElementById !== "function" || typeof id !== "string" || !id
    || typeof element.addEventListener !== "function" || typeof element.removeEventListener !== "function"
    || typeof element.dispatchEvent !== "function"
    || (kind === "button" ? tag !== "BUTTON" || (element as HTMLButtonElement).type !== "button"
      : tag !== "TEXTAREA" && (tag !== "INPUT" || !["text", "search", "url", "tel", "email", "password"].includes(inputType!)))) {
    fail("invalid_dom_control_element");
  }
  const EventConstructor = document.defaultView?.Event;
  if (typeof EventConstructor !== "function") fail("invalid_dom_control_document");
  const eventType = kind === "button" ? "click" : "input";
  const owner = {};
  const ownedEvents = bindings.get(element) ?? new Set<string>();
  if (ownedEvents.has(eventType)) fail("dom_control_already_bound");
  const seen = new WeakSet<Event>();
  const manual = new AbortController();
  let disposed = false, dispatching: Invocation | undefined, handling: Invocation | undefined;

  function current(): void {
    if (disposed) fail("dom_control_disposed");
    if (!element.isConnected || element.ownerDocument !== document || element.id !== id
      || document.getElementById(id) !== element || element.tagName !== tag
      || (kind === "button" && (element as HTMLButtonElement).type !== "button")
      || (inputType !== undefined && (element as HTMLInputElement).type !== inputType)) {
      fail("registered DOM instance has been replaced");
    }
  }
  function actionable(): void {
    current();
    if (element.disabled || (kind === "text" && (element as HTMLInputElement).readOnly)) fail("dom_control_not_actionable");
  }
  function reject(invocation: Invocation, error: unknown): void {
    if (!invocation.failed) { invocation.failed = true; invocation.error = error; }
    invocation.finished = true;
    if (!invocation.notified) {
      invocation.notified = true;
      try { asyncResult(onError?.(invocation.error, invocation.origin)); } catch { /* Display callbacks cannot recover an action. */ }
    }
  }
  function context(invocation: Invocation, original?: RegisteredActionContext): RegisteredActionContext {
    let committing = false;
    const signal = original?.signal ?? manual.signal;
    const check = () => {
      if (invocation.failed) throw invocation.error;
      if (invocation.finished) fail("dom_action_finished");
      if (signal.aborted) fail("dom_action_cancelled");
      current();
    };
    invocation.check = check;
    invocation.actionable = () => { check(); actionable(); };
    return Object.freeze({ signal,
      commit<T>(mutation: () => T): T {
        check();
        if (committing) fail("reentrant_commit");
        const run = () => {
          committing = true;
          try {
            check(); actionable();
            if (asyncResult(beforeCommit?.())) fail("async_dom_before_commit");
            check(); actionable();
            const result = mutation();
            if (asyncResult(result)) fail("async_commit");
            return result;
          } finally { committing = false; }
        };
        return original ? original.commit(run) : run();
      },
      changed(...names: string[]): void {
        check();
        if (!committing) fail("registered_change_outside_commit");
        if (original) original.changed(...names); else changed(...names);
      },
    });
  }
  function invocation(event: Event, origin: DomEventOrigin, original?: RegisteredActionContext): Invocation {
    const value = { event, origin, consumed: false, failed: false, notified: false, finished: false } as Invocation;
    value.context = context(value, original);
    return value;
  }
  function receive(event: Event): void {
    if (disposed) return;
    if (issuing && (issuing.owner !== owner || issuing.event !== event)) {
      issuing.reject(new Error("dom_event_reentry"));
      return;
    }
    const active = dispatching ?? handling;
    if (handling || (dispatching && (event !== dispatching.event || dispatching.consumed)) || seen.has(event)) {
      if (active) reject(active, new Error("dom_event_reentry"));
      else reject(invocation(event, "manual"), new Error("dom_event_reentry"));
      return;
    }
    const captured = dispatching ?? invocation(event, "manual");
    captured.consumed = true;
    seen.add(event);
    handling = captured;
    try {
      captured.actionable();
      const result = handler(captured);
      if (result !== undefined) {
        if (!(result instanceof Promise)) fail("invalid_dom_handler_result");
        captured.completion = Promise.prototype.then.call(result, (value: unknown) => {
          captured.finished = true;
          if (value !== undefined) fail("invalid_dom_handler_result");
        }, (error: unknown) => { captured.finished = true; throw error; }) as Promise<void>;
        // DOM event dispatch does not consume returned promises. Keep the exact
        // invocation's completion while also handling manual async failures.
        void captured.completion.catch((error) => reject(captured, error));
      } else captured.finished = true;
    } catch (error) { reject(captured, error); }
    finally { handling = undefined; }
  }
  ownedEvents.add(eventType);
  bindings.set(element, ownedEvents);
  try { element.addEventListener(eventType, receive); }
  catch (error) { ownedEvents.delete(eventType); if (!ownedEvents.size) bindings.delete(element); throw error; }
  return {
    read(): ControlState { current(); const value = read(); current(); return value; },
    async dispatch(original: RegisteredActionContext, prepare?: () => void, text?: string): Promise<void> {
      actionable();
      if (dispatching || handling) fail("dom_event_reentry");
      if (issuing) {
        const error = new Error("dom_event_reentry");
        issuing.reject(error);
        throw error;
      }
      const event = new EventConstructor(eventType, { bubbles: true, cancelable: true });
      const captured = invocation(event, "agent", original);
      captured.text = text;
      dispatching = captured;
      issuing = { event, owner, reject: (error) => reject(captured, error) };
      try {
        const emit = () => {
          if (prepare) { prepare(); captured.actionable(); (element as HTMLInputElement).value = text!; }
          element.dispatchEvent(event);
        };
        if (prepare) captured.context.commit(emit); else emit();
      } catch (error) { reject(captured, error); }
      finally { dispatching = undefined; if (issuing?.owner === owner) issuing = undefined; }
      if (captured.failed) throw captured.error;
      if (!captured.consumed) fail("dom_handler_not_run");
      await captured.completion;
      if (captured.failed) throw captured.error;
      if (original.signal.aborted) fail("dom_action_cancelled");
      current();
    },
    dispose(): void {
      if (disposed) return;
      disposed = true;
      manual.abort();
      element.removeEventListener(eventType, receive);
      ownedEvents.delete(eventType);
      if (!ownedEvents.size) bindings.delete(element);
    },
  };
}

/** Bind an application-owned button handler; every mutation remains explicit. */
export function createDomButtonControl(options: DomButtonControlOptions): DomControlBinding {
  requireFunction(options?.onClick);
  const onClick = options.onClick.bind(options);
  const binding = bind(options.element, options, "button", ({ context }) => onClick(context), options.beforeCommit?.bind(options));
  return Object.freeze({ control: Object.freeze({ read: binding.read,
    click: (context: RegisteredActionContext) => binding.dispatch(context) }), dispose: binding.dispose });
}

/** Assign an agent value and run the same synchronous input handler in one commit. */
export function createDomTextControl(options: DomTextControlOptions): DomControlBinding {
  requireFunction(options?.onInput);
  if (options.validateText !== undefined) requireFunction(options.validateText);
  const onInput = options.onInput.bind(options), validateText = options.validateText?.bind(options), element = options.element;
  const binding = bind(element, options, "text", (invocation) => {
    const input = () => {
      const text = element.value;
      if (invocation.origin === "agent") {
        if (text !== invocation.text) fail("dom_input_value_changed");
      } else if (asyncResult(validateText?.(text))) fail("async_dom_text_validation");
      invocation.actionable();
      if (element.value !== text) fail("dom_input_value_changed");
      const value = onInput(text, Object.freeze({ signal: invocation.context.signal, changed: invocation.context.changed }));
      if (value !== undefined) { asyncResult(value); fail("invalid_dom_handler_result"); }
    };
    if (invocation.origin === "agent") input(); else invocation.context.commit(input);
  });
  return Object.freeze({ control: Object.freeze({ read: binding.read,
    setText(text: string, context: RegisteredActionContext): Promise<void> {
      return binding.dispatch(context, () => {
        if (typeof text !== "string") fail("invalid_dom_text");
        if (asyncResult(validateText?.(text))) fail("async_dom_text_validation");
      }, text);
    } }), dispose: binding.dispose });
}
