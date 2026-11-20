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

/** Fixed logical names whose mounted control instances may change explicitly. */
export interface DynamicRegisteredApplication extends RegisteredApplication {
  /** Notify mounted dependencies; declared empty slots are valid and skipped. */
  changed(...names: string[]): void;
  /** Populate a declared empty slot. Names cannot be added after construction. */
  mount(name: string, control: RegisteredControl): void;
  /** Replace a mounted instance, including a same-value replacement. */
  replace(name: string, control: RegisteredControl): void;
  /** Retire a mounted instance without removing its declared logical slot. */
  unmount(name: string): void;
  /** Local structural identity; ordinary state changes and attachments do not increment it. */
  bindingRevision(): number;
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

function declaration(name: string, control: unknown): Declaration {
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
}

/**
 * Fixed registered objects with application-owned readers and action handlers.
 * Mutations remain explicit synchronous commit callbacks, including after any
 * await. No scheduler, retry, rollback or cached application evidence is added.
 */
export function createRegisteredApplication(
  controls: Readonly<Record<string, RegisteredControl>>,
): RegisteredApplication {
  return applicationEngine(controls, false).application;
}

/**
 * Explicit mounting for at most 64 predeclared names. Structural changes never
 * retry an action, reuse an old generation or roll back domain side effects.
 */
export function createDynamicRegisteredApplication(
  slots: Readonly<Record<string, RegisteredControl | null>>,
): DynamicRegisteredApplication {
  const engine = applicationEngine(slots, true);
  return Object.freeze({ ...engine.application, mount: engine.mount, replace: engine.replace,
    unmount: engine.unmount, bindingRevision: engine.bindingRevision });
}

function applicationEngine(controls: Readonly<Record<string, RegisteredControl | null>>, dynamic: boolean) {
  if (!controls || typeof controls !== "object" || Array.isArray(controls)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(controls))) invalid();
  const keys = Reflect.ownKeys(controls);
  if (keys.length > 64 || keys.some((name) => typeof name !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(name))) invalid();
  const slots = new Map<string, Declaration | null>(keys.map((key): [string, Declaration | null] => {
    const name = key as string;
    const field = Object.getOwnPropertyDescriptor(controls, name);
    if (!field || !("value" in field) || !field.enumerable) invalid();
    return [name, dynamic && field.value === null ? null : declaration(name, field.value)];
  }));
  let active: Attachment | null = null;
  let revision = 0;
  let busy = false;
  let readers = 0;

  function available(): void {
    if (dynamic && (busy || readers)) throw new Error("registered_application_busy");
  }
  function mounted(): Declaration[] {
    return [...slots.values()].filter((value): value is Declaration => value !== null);
  }
  function read(declared: Declaration): ControlState {
    const before = revision;
    readers++;
    try {
      const value = declared.read();
      if (dynamic && revision !== before) throw new Error("registered_application_bindings_changed");
      return value;
    } finally { readers--; }
  }

  function checkedNames(values: string[]): string[] {
    if (values.some((name) => typeof name !== "string" || !slots.has(name))) {
      throw new Error("unknown_registered_control");
    }
    return [...new Set(values)];
  }
  function requireAttached(attachment: Attachment): void {
    if (active !== attachment || attachment.detached || attachment.bridge.closed) {
      throw new Error("registered_application_detached");
    }
  }
  function actionContext(attachment: Attachment, original: ActionContext, declared: Declaration): RegisteredActionContext {
    const capturedRevision = revision;
    let committing = false;
    function current(): void {
      requireAttached(attachment);
      if (dynamic && (revision !== capturedRevision || slots.get(declared.name) !== declared)) {
        throw new Error("registered_application_bindings_changed");
      }
    }
    return Object.freeze({
      signal: original.signal,
      commit<T>(mutation: () => T): T {
        current();
        return original.commit(() => {
          current();
          committing = true;
          try { return mutation(); }
          finally { committing = false; }
        });
      },
      changed(...values: string[]): void {
        const selected = checkedNames(values);
        if (!committing) throw new Error("registered_change_outside_commit");
        current();
        if (original.signal.aborted) throw new Error("registered_action_cancelled");
        // The enclosing original.commit supplied authority and revision gates.
        // Reentering it here would reject both nesting and self-disabling actions.
        for (const name of selected) if (slots.get(name)) attachment.bridge.changed(name);
      },
    });
  }

  function adapter(attachment: Attachment, declared: Declaration): Adapter {
    const value: Adapter = { read: () => read(declared) };
    if (declared.setText) value.setText = (text, context) =>
      declared.setText!(text, actionContext(attachment, context, declared));
    if (declared.click) value.click = (context) =>
      declared.click!(actionContext(attachment, context, declared));
    return value;
  }

  function retire(attachment: Attachment): void {
    attachment.detached = true;
    try { attachment.bridge.close(); }
    finally { if (active === attachment && !attachment.attaching) active = null; }
  }

  function change(kind: "mount" | "replace" | "unmount", name: string, control?: RegisteredControl): void {
    available();
    if (typeof name !== "string" || !slots.has(name)) throw new Error("unknown_registered_control");
    const previous = slots.get(name) ?? null;
    if (kind === "mount" ? previous !== null : previous === null) {
      throw new Error(kind === "mount" ? "registered_control_already_mounted" : "registered_control_not_mounted");
    }
    if (revision === Number.MAX_SAFE_INTEGER) throw new Error("registered_binding_revision_exhausted");
    busy = true;
    let attachment: Attachment | null = null;
    try {
      const next = kind === "unmount" ? null : declaration(name, control);
      attachment = active;
      if (attachment) {
        requireAttached(attachment);
        if (kind === "mount") attachment.bridge.register(name, adapter(attachment, next!));
        else if (kind === "replace") attachment.bridge.replace(name, adapter(attachment, next!));
        else attachment.bridge.unregister(name);
        requireAttached(attachment);
      }
      // No application callback can run between these two publication writes.
      slots.set(name, next);
      revision++;
    } catch (error) {
      // A Bridge failure can have invalidated an entry before throwing. Never
      // leave a live session with mismatched tables; keep declarations recoverable.
      if (attachment) retire(attachment);
      throw error;
    } finally { busy = false; }
  }

  const application: RegisteredApplication = Object.freeze({
    attach(bridge: Bridge): void {
      if (active) throw new Error("registered_application_already_attached");
      available();
      if (!(bridge instanceof Bridge) || bridge.closed) throw new Error("registered_application_detached");
      // Keep this slot through all reader callbacks, even a reentrant detach.
      const attachment: Attachment = { bridge, attaching: true, detached: false };
      active = attachment;
      if (dynamic) busy = true;
      try {
        for (const declared of mounted()) {
          bridge.register(declared.name, adapter(attachment, declared));
          requireAttached(attachment);
        }
        requireAttached(attachment);
      } catch (error) {
        bridge.close();
        if (active === attachment) active = null;
        throw error;
      } finally { attachment.attaching = false; if (dynamic) busy = false; }
    },
    detach(bridge: Bridge): void {
      if (dynamic && active?.bridge === bridge) active.detached = true;
      bridge.close();
      if (active?.bridge === bridge) {
        active.detached = true;
        if (!active.attaching) active = null;
      }
    },
    observe(): RegisteredApplicationObservation[] {
      available();
      const declarations = mounted(), before = revision;
      if (dynamic) busy = true;
      try {
        const rows = declarations.map((declared) => {
          const { name } = declared;
          const value = read(declared);
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
        if (dynamic && revision !== before) throw new Error("registered_application_bindings_changed");
        return declarations.map(({ name }) => byName.get(name)!);
      } finally { if (dynamic) busy = false; }
    },
    changed(...values: string[]): void {
      const selected = checkedNames(values);
      available();
      if (!active) return;
      const attachment = active;
      requireAttached(attachment);
      if (attachment.attaching) throw new Error("registered_application_attaching");
      for (const name of selected) if (slots.get(name)) attachment.bridge.changed(name);
    },
  });
  return { application,
    mount: (name: string, control: RegisteredControl) => change("mount", name, control),
    replace: (name: string, control: RegisteredControl) => change("replace", name, control),
    unmount: (name: string) => change("unmount", name),
    bindingRevision: () => revision };
}
