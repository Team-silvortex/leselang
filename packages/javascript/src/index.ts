export { AgentSession } from "./agent.js";
export { prepareSemanticCatalog } from "./semantics.js";
export type { SemanticCatalog, SemanticTaskInfo, PreparedSemanticTask } from "./semantics.js";
export { createSemanticProvider } from "./semantic-provider.js";
export type { SemanticProviderOptions } from "./semantic-provider.js";
export type {
  AgentContext, AgentHost, AgentJson, AgentOutcome, AgentPhase, AgentProvider,
  AgentSessionOptions, AgentState, WorkflowPlan,
} from "./agent.js";

/** A bounded host adapter, not an evaluator or a source-code execution API. */
export interface Scope {
  window: string;
  epoch: string;
  session: string;
}

export interface ControlState {
  role: string;
  title: string;
  value: string;
  enabled: boolean;
  editable: boolean;
  clicks: number;
}

export interface ActionContext {
  readonly signal: AbortSignal;
  /** Every asynchronous adapter must re-enter this gate before mutating state. */
  commit<T>(mutation: () => T): T;
}

export interface Adapter {
  read(): ControlState;
  setText?(text: string, context: ActionContext): void | Promise<void>;
  click?(context: ActionContext): void | Promise<void>;
}

export interface Target {
  name: string;
  generation: number;
}

export interface Snapshot extends ControlState {
  identifier: string;
  generation: number;
  revision: number;
}

export interface Request extends Scope {
  transport_version: 1;
  request_id: number;
  budget_ms: number;
  operation: "gui.list" | "gui.find" | "gui.observe" | "gui.set_text" | "gui.click";
  arguments: Record<string, unknown>;
}

export interface Reply extends Scope {
  transport_version: 1;
  request_id: number;
  status: "ok" | "rejected" | "uncertain";
  value: string[] | Target | Snapshot | boolean | null;
  error: string | null;
}

interface Entry {
  name: string;
  generation: number;
  revision: number;
  adapter: Adapter | null;
  observed: ControlState;
}

interface Pending {
  request: Request;
  controller: AbortController;
  deadline: number;
  entry: Entry | null;
  generation: number;
  revision: number;
  invoked: boolean;
  committing: boolean;
}

const U32_MAX = 0xffff_ffff;
const MAX_OBJECTS = 64;
const MAX_TEXT_BYTES = 4096;
const MAX_BUDGET_MS = 5000;
const encoder = new TextEncoder();

class Fault extends Error {
  readonly code: string;
  constructor(code: string) {
    super(code);
    this.code = code;
  }
}

function reject(code: string): never {
  throw new Fault(code);
}

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    reject("malformed_request");
  }
  return value as Record<string, unknown>;
}

function exact(value: Record<string, unknown>, keys: string[]): void {
  const actual = Object.keys(value);
  if (actual.length !== keys.length || actual.some((key) => !keys.includes(key))) {
    reject("malformed_request");
  }
}

function integer(value: unknown, minimum = 0, maximum = U32_MAX): number {
  if (!Number.isInteger(value) || typeof value !== "number" || value < minimum || value > maximum) {
    reject("malformed_request");
  }
  return value;
}

function text(value: unknown, limit = MAX_TEXT_BYTES): string {
  if (typeof value !== "string" || encoder.encode(value).length > limit) {
    reject("malformed_request");
  }
  // JSON cannot transport isolated UTF-16 surrogates into Rust String faithfully.
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) reject("malformed_request");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      reject("malformed_request");
    }
  }
  return value;
}

function key(value: unknown): string {
  const result = text(value, 64);
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(result)) reject("malformed_request");
  return result;
}

function state(value: ControlState): ControlState {
  try {
    if (typeof value.enabled !== "boolean" || typeof value.editable !== "boolean") {
      reject("invalid_state");
    }
    return Object.freeze({
      role: text(value.role, 64),
      title: text(value.title),
      value: text(value.value),
      enabled: value.enabled,
      editable: value.editable,
      clicks: integer(value.clicks),
    });
  } catch {
    return reject("invalid_state");
  }
}

function same(left: ControlState, right: ControlState): boolean {
  return left.role === right.role && left.title === right.title && left.value === right.value &&
    left.enabled === right.enabled && left.editable === right.editable && left.clicks === right.clicks;
}

/** One trusted application endpoint and one execution session own this bridge. */
export class Bridge {
  readonly scope: Readonly<Scope>;
  #entries = new Map<string, Entry>();
  #lastRequest = 0;
  #pending: Pending | null = null;
  #live = true;
  #granted = true;

  constructor(scope: Scope) {
    this.scope = Object.freeze({ window: key(scope.window), epoch: key(scope.epoch), session: key(scope.session) });
  }

  register(name: string, adapter: Adapter): void {
    this.#requireLive();
    key(name);
    const previous = this.#entries.get(name);
    if (previous?.adapter) reject("duplicate_object");
    if (!previous && this.#entries.size >= MAX_OBJECTS) reject("object_limit");
    const observed = state(adapter.read());
    const generation = previous ? this.#nextGeneration(previous) : 1;
    this.#entries.set(name, { name, generation, revision: 0, adapter, observed });
  }

  replace(name: string, adapter: Adapter): void {
    this.#requireLive();
    const previous = this.#entry(name);
    const observed = state(adapter.read());
    const generation = this.#nextGeneration(previous);
    // A distinct entry also fences callbacks retaining the previous adapter.
    this.#entries.set(name, { name, generation, revision: 0, adapter, observed });
  }

  unregister(name: string): void {
    this.#requireLive();
    const entry = this.#entry(name);
    entry.generation = this.#nextGeneration(entry);
    entry.adapter = null;
  }

  /** Record model changes, including ABA or dependencies invisible to read(). */
  changed(name: string): void {
    this.#requireLive();
    this.#advance(this.#entry(name));
  }

  cancel(session: string): void {
    if (session !== this.scope.session) return;
    this.#live = false;
    this.#pending?.controller.abort();
  }

  revoke(): void {
    this.#granted = false;
    this.#pending?.controller.abort();
  }

  close(): void {
    this.#live = false;
    this.#pending?.controller.abort();
  }

  async handle(input: unknown): Promise<Reply> {
    let requestId = 0;
    let pending: Pending | null = null;
    try {
      const envelope = record(input);
      if (typeof envelope.request_id === "number" && Number.isInteger(envelope.request_id) &&
        envelope.request_id >= 1 && envelope.request_id <= U32_MAX) requestId = envelope.request_id;
      const request = this.#request(input);
      requestId = request.request_id;
      this.#requireLive();
      if (!this.#granted) reject("revoked");
      if (this.#pending) reject("busy");
      if (request.request_id !== this.#lastRequest + 1) reject("request_order");
      this.#lastRequest = request.request_id;
      pending = {
        request, controller: new AbortController(), deadline: performance.now() + request.budget_ms,
        entry: null, generation: 0, revision: 0, invoked: false, committing: false,
      };
      this.#pending = pending;
      const value = await this.#dispatch(pending);
      this.#fence(pending);
      return this.#reply(requestId, "ok", value, null);
    } catch (error) {
      return this.#reply(requestId, pending?.invoked ? "uncertain" : "rejected", null,
        error instanceof Fault ? error.code : "adapter_failed");
    } finally {
      if (pending && this.#pending === pending) this.#pending = null;
    }
  }

  #request(input: unknown): Request {
    const request = record(input);
    exact(request, ["transport_version", "window", "epoch", "session", "request_id", "budget_ms", "operation", "arguments"]);
    if (request.transport_version !== 1) reject("transport_version");
    integer(request.request_id, 1);
    integer(request.budget_ms, 1, MAX_BUDGET_MS);
    if (request.window !== this.scope.window) reject("wrong_window");
    if (request.epoch !== this.scope.epoch) reject("wrong_epoch");
    if (request.session !== this.scope.session) reject("wrong_session");
    const args = record(request.arguments);
    let admitted: Record<string, unknown>;
    switch (request.operation) {
      case "gui.list": exact(args, []); admitted = {}; break;
      case "gui.find": exact(args, ["name"]); admitted = { name: key(args.name) }; break;
      case "gui.observe": exact(args, ["target"]); admitted = { target: this.#target(args.target) }; break;
      case "gui.set_text":
        exact(args, ["target", "expected_revision", "text"]);
        admitted = { target: this.#target(args.target), expected_revision: integer(args.expected_revision), text: text(args.text) }; break;
      case "gui.click":
        exact(args, ["target", "expected_revision"]);
        admitted = { target: this.#target(args.target), expected_revision: integer(args.expected_revision) }; break;
      default: reject("unknown_operation");
    }
    // Own admitted data before yielding to callbacks; callers cannot edit an in-flight request.
    return {
      transport_version: 1, ...this.scope, request_id: request.request_id as number,
      budget_ms: request.budget_ms as number, operation: request.operation,
      arguments: admitted,
    };
  }

  #target(value: unknown): Target {
    const target = record(value);
    exact(target, ["name", "generation"]);
    return { name: key(target.name), generation: integer(target.generation, 1) };
  }

  #reply(requestId: number, status: Reply["status"], value: Reply["value"], error: string | null): Reply {
    return { transport_version: 1, ...this.scope, request_id: requestId, status, value, error };
  }

  async #dispatch(pending: Pending): Promise<Reply["value"]> {
    const { operation, arguments: args } = pending.request;
    if (operation === "gui.list") {
      return [...this.#entries.values()].filter((entry) => entry.adapter !== null).map((entry) => entry.name);
    }
    if (operation === "gui.find") {
      const entry = this.#entry(args.name as string);
      this.#refresh(entry);
      return { name: entry.name, generation: entry.generation };
    }
    const target = this.#target(args.target);
    const entry = this.#entry(target.name);
    if (entry.generation !== target.generation) reject("stale_object");
    const observed = this.#refresh(entry);
    if (operation === "gui.observe") return this.#snapshot(entry, observed);
    if (entry.revision !== args.expected_revision) reject("stale_revision");
    if (!observed.enabled) reject("disabled");
    if (operation === "gui.set_text" && (observed.role !== "text_field" || !observed.editable || !entry.adapter?.setText)) {
      reject("not_editable");
    }
    if (operation === "gui.click" && (observed.role !== "button" || !entry.adapter?.click)) {
      reject("not_clickable");
    }
    // Admission fences re-use even when invocation subsequently rejects.
    this.#advance(entry);
    pending.entry = entry;
    pending.generation = entry.generation;
    pending.revision = entry.revision;
    const context: ActionContext = Object.freeze({
      signal: pending.controller.signal,
      commit: <T>(mutation: () => T): T => {
        this.#fence(pending);
        if (pending.committing) reject("reentrant_commit");
        this.#advance(entry);
        pending.revision = entry.revision;
        pending.committing = true;
        try {
          const result = mutation();
          if (result && typeof (result as { then?: unknown }).then === "function") reject("async_commit");
          // The synchronous callback can call changed() for its own event handlers.
          if (!entry.adapter || this.#entries.get(entry.name) !== entry || entry.generation !== pending.generation) {
            reject("stale_object");
          }
          this.#refresh(entry);
          pending.revision = entry.revision;
          return result;
        } finally {
          pending.committing = false;
        }
      },
    });
    let timer: ReturnType<typeof setTimeout> | undefined;
    let abort: (() => void) | undefined;
    try {
      const interrupted = new Promise<never>((_, fail) => {
        timer = setTimeout(() => {
          // Timer rounding can invoke this callback just before the fractional
          // deadline. Settle its explicit cause before the abort listener runs.
          fail(new Fault("deadline"));
          pending.controller.abort();
        }, Math.max(0, pending.deadline - performance.now()));
        abort = () => fail(new Fault(performance.now() >= pending.deadline ? "deadline" : "cancelled"));
        pending.controller.signal.addEventListener("abort", abort, { once: true });
      });
      const completion = Promise.resolve().then(() => {
        this.#fence(pending);
        pending.invoked = true;
        return operation === "gui.set_text"
          ? entry.adapter!.setText!(args.text as string, context)
          : entry.adapter!.click!(context);
      });
      await Promise.race([completion, interrupted]);
      this.#fence(pending);
      return true;
    } finally {
      if (timer !== undefined) clearTimeout(timer);
      if (abort) pending.controller.signal.removeEventListener("abort", abort);
    }
  }

  #snapshot(entry: Entry, observed: ControlState): Snapshot {
    return { identifier: entry.name, generation: entry.generation, revision: entry.revision, ...observed };
  }

  #fence(pending: Pending): void {
    this.#requireLive();
    if (!this.#granted) reject("revoked");
    if (this.#pending !== pending || pending.controller.signal.aborted) reject("cancelled");
    if (performance.now() >= pending.deadline) reject("deadline");
    if (pending.entry) {
      const entry = pending.entry;
      if (this.#entries.get(entry.name) !== entry || !entry.adapter || entry.generation !== pending.generation) {
        reject("stale_object");
      }
      this.#refresh(entry);
      if (entry.revision !== pending.revision) reject("stale_revision");
      if (!entry.observed.enabled) reject("disabled");
      if (pending.request.operation === "gui.set_text" && !entry.observed.editable) reject("not_editable");
    }
  }

  #entry(name: string): Entry {
    const entry = this.#entries.get(name);
    if (!entry?.adapter) reject("unknown_object");
    return entry;
  }

  #refresh(entry: Entry): ControlState {
    if (!entry.adapter) reject("stale_object");
    const observed = state(entry.adapter.read());
    if (!same(observed, entry.observed)) {
      this.#advance(entry);
      entry.observed = observed;
    }
    return observed;
  }

  #nextGeneration(entry: Entry): number {
    if (entry.generation === U32_MAX) {
      entry.adapter = null;
      return reject("generation_exhausted");
    }
    return entry.generation + 1;
  }

  #advance(entry: Entry): void {
    if (entry.revision === U32_MAX) {
      entry.adapter = null;
      reject("revision_exhausted");
    }
    entry.revision += 1;
  }

  #requireLive(): void {
    if (!this.#live) reject("closed");
  }
}
