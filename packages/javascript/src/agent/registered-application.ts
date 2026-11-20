import { Bridge, type ActionContext, type Adapter, type ControlState } from "../bridge.js";
import type { RegisteredApplicationConnection } from "./registered-host.js";
import { snapshots } from "./registered-evidence.js";

export interface RegisteredActionContext extends ActionContext {
  /** Notify dependencies inside this context's synchronous commit callback. */
  changed(...names: string[]): void;
}

export interface RegisteredControl {
  read(): ControlState;
  setText?(text: string, context: RegisteredActionContext): void | Promise<void>;
  click?(context: RegisteredActionContext): void | Promise<void>;
}

export interface RegisteredApplicationObservation extends ControlState {
  readonly identifier: string;
}

export interface RegisteredApplication extends RegisteredApplicationConnection {
  /** Read current application state, independently of any bridge's cache. */
  observe(): RegisteredApplicationObservation[];
  /** Notify current-session revisions after a manual application mutation. */
  changed(...names: string[]): void;
}

interface Declaration {
  readonly name: string;
  readonly read: () => ControlState;
  readonly setText?: (text: string, context: RegisteredActionContext) => void | Promise<void>;
  readonly click?: (context: RegisteredActionContext) => void | Promise<void>;
}

interface Attachment {
  readonly bridge: Bridge;
  attaching: boolean;
  detached: boolean;
}

function invalid(): never { throw new Error("invalid_registered_application_controls"); }

/**
 * Fixed registered objects with application-owned readers and action handlers.
 * Mutations remain explicit synchronous commit callbacks, including after any
 * await. No scheduler, retry, rollback or cached application evidence is added.
 */
export function createRegisteredApplication(
  controls: Readonly<Record<string, RegisteredControl>>,
): RegisteredApplication {
  if (!controls || typeof controls !== "object" || Array.isArray(controls)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(controls))) invalid();
  const keys = Reflect.ownKeys(controls);
  if (keys.length > 64 || keys.some((name) => typeof name !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(name))) invalid();
  const declarations: Declaration[] = keys.map((key) => {
    const name = key as string;
    const field = Object.getOwnPropertyDescriptor(controls, name);
    if (!field || !("value" in field) || !field.enumerable) invalid();
    const control: unknown = field.value;
    if (!control || typeof control !== "object" || Array.isArray(control)
      || ![Object.prototype, null].includes(Object.getPrototypeOf(control))) invalid();
    const methods = Object.getOwnPropertyDescriptors(control);
    if (Reflect.ownKeys(control).some((method) => !["read", "setText", "click"].includes(method as string))) invalid();
    for (const method of ["read", "setText", "click"]) {
      const descriptor = methods[method];
      if (descriptor && (!("value" in descriptor) || typeof descriptor.value !== "function")) invalid();
    }
    if (!methods.read) invalid();
    // Capture function identities once without calling any application reader.
    return Object.freeze({ name, read: methods.read.value.bind(control),
      setText: methods.setText?.value.bind(control), click: methods.click?.value.bind(control) });
  });
  const names = new Set(declarations.map(({ name }) => name));
  let active: Attachment | null = null;

  function checkedNames(values: string[]): string[] {
    if (values.some((name) => typeof name !== "string" || !names.has(name))) {
      throw new Error("unknown_registered_control");
    }
    return [...new Set(values)];
  }
  function requireAttached(attachment: Attachment): void {
    if (active !== attachment || attachment.detached || attachment.bridge.closed) {
      throw new Error("registered_application_detached");
    }
  }
  function actionContext(attachment: Attachment, original: ActionContext): RegisteredActionContext {
    let committing = false;
    return Object.freeze({
      signal: original.signal,
      commit<T>(mutation: () => T): T {
        requireAttached(attachment);
        return original.commit(() => {
          requireAttached(attachment);
          committing = true;
          try { return mutation(); }
          finally { committing = false; }
        });
      },
      changed(...values: string[]): void {
        const selected = checkedNames(values);
        if (!committing) throw new Error("registered_change_outside_commit");
        requireAttached(attachment);
        if (original.signal.aborted) throw new Error("registered_action_cancelled");
        // The enclosing original.commit supplied authority and revision gates.
        // Reentering it here would reject both nesting and self-disabling actions.
        for (const name of selected) attachment.bridge.changed(name);
      },
    });
  }

  return Object.freeze({
    attach(bridge: Bridge): void {
      if (active) throw new Error("registered_application_already_attached");
      if (!(bridge instanceof Bridge) || bridge.closed) throw new Error("registered_application_detached");
      // Keep this slot through all reader callbacks, even a reentrant detach.
      const attachment: Attachment = { bridge, attaching: true, detached: false };
      active = attachment;
      try {
        for (const declaration of declarations) {
          const adapter: Adapter = { read: declaration.read };
          if (declaration.setText) adapter.setText = (text, context) =>
            declaration.setText!(text, actionContext(attachment, context));
          if (declaration.click) adapter.click = (context) =>
            declaration.click!(actionContext(attachment, context));
          bridge.register(declaration.name, adapter);
          requireAttached(attachment);
        }
        requireAttached(attachment);
      } catch (error) {
        bridge.close();
        if (active === attachment) active = null;
        throw error;
      } finally { attachment.attaching = false; }
    },
    detach(bridge: Bridge): void {
      bridge.close();
      if (active?.bridge === bridge) {
        active.detached = true;
        if (!active.attaching) active = null;
      }
    },
    observe(): RegisteredApplicationObservation[] {
      const rows = declarations.map(({ name, read }) => {
        const value = read();
        if (!value || typeof value !== "object" || Array.isArray(value)
          || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
          throw new Error("invalid_application_observation");
        }
        const fields = Object.getOwnPropertyDescriptors(value);
        if (Object.hasOwn(fields, "identifier")) throw new Error("invalid_application_observation");
        // Preserve descriptors until evidence validation so accessors are refused
        // without executing them or turning them into trusted plain evidence.
        return Object.create(Object.getPrototypeOf(value), { ...fields,
          identifier: { enumerable: true, value: name } });
      });
      const owned = JSON.parse(snapshots(rows)) as RegisteredApplicationObservation[];
      if (owned.some((row) => new TextEncoder().encode(row.role).length > 64)) {
        throw new Error("invalid_application_observation");
      }
      const byName = new Map(owned.map((row) => [row.identifier, row]));
      return declarations.map(({ name }) => byName.get(name)!);
    },
    changed(...values: string[]): void {
      const selected = checkedNames(values);
      if (!active) return;
      const attachment = active;
      requireAttached(attachment);
      if (attachment.attaching) throw new Error("registered_application_attaching");
      for (const name of selected) attachment.bridge.changed(name);
    },
  });
}
