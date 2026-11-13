/** Application-owned planning and execution; this layer never polls a VM. */
export type AgentJson = null | boolean | number | string |
  readonly AgentJson[] | { readonly [key: string]: AgentJson };

export interface WorkflowPlan {
  readonly summary: string;
  readonly source: string;
  /** Optional declared task id; existing providers may omit it. */
  readonly task?: string;
  /** Zero or one named string input. The host admits its supported task shape. */
  readonly arguments: Readonly<Record<string, string>>;
}

export interface AgentContext {
  readonly taskId: number;
  readonly message: string;
  readonly catalog: AgentJson;
}

export interface AgentProvider {
  /** Propose source and literal arguments. A proposal is not an authority grant. */
  plan(context: AgentContext, signal: AbortSignal): Promise<WorkflowPlan>;
}

export interface AgentHost {
  describe(signal: AbortSignal): Promise<unknown>;
  /** Own compilation, grants, explicit VM driving and transport cancellation. */
  execute(plan: WorkflowPlan, signal: AbortSignal): Promise<unknown>;
  /** Application verification is separate from the provider's claims. */
  verify(report: AgentJson, plan: WorkflowPlan, signal: AbortSignal): boolean | Promise<boolean>;
}

export type AgentPhase = "idle" | "describing" | "planning" | "executing" |
  "verifying" | "cancelling" | "verified" | "failed" | "cancelled";

export interface AgentState {
  readonly taskId: number;
  readonly phase: AgentPhase;
  readonly possiblePartialEffects: boolean;
  readonly summary?: string;
  readonly error?: string;
}

export interface AgentOutcome {
  readonly taskId: number;
  readonly status: "verified" | "failed" | "cancelled";
  readonly possiblePartialEffects: boolean;
  readonly plan?: WorkflowPlan;
  readonly report?: AgentJson;
  readonly error?: string;
}

export interface AgentSessionOptions {
  readonly provider: AgentProvider;
  readonly host: AgentHost;
  /** Display-only observer. Throwing here does not change execution authority. */
  readonly onEvent?: (state: AgentState) => void;
}

interface Task {
  readonly id: number;
  readonly controller: AbortController;
  enteredExecute: boolean;
  plan?: WorkflowPlan;
  report?: AgentJson;
}

const encoder = new TextEncoder();
const JSON_LIMIT = 128 * 1024;
const CATALOG_LIMIT = 256 * 1024;
const JSON_NODE_LIMIT = 8192;

function fault(message: string): never { throw new Error(message); }

function boundedText(value: unknown, limit: number, label: string, nonempty = false): string {
  if (typeof value !== "string" || (nonempty && value.trim().length === 0) || value.length > limit) {
    return fault(`invalid_${label}`);
  }
  // Isolated UTF-16 surrogates do not round-trip as Rust UTF-8 strings.
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) fault(`invalid_${label}`);
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      fault(`invalid_${label}`);
    }
  }
  if (encoder.encode(value).length > limit) fault(`invalid_${label}`);
  return value;
}

function fields(value: unknown, expected?: readonly string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value) ||
      ![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
    return fault("invalid_agent_record");
  }
  const keys = Reflect.ownKeys(value);
  if (keys.some((key) => typeof key !== "string") ||
      (expected && (keys.length !== expected.length || keys.some((key) => !expected.includes(key as string))))) {
    fault("invalid_agent_record");
  }
  const result: Record<string, unknown> = Object.create(null);
  for (const key of keys as string[]) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (!descriptor || !("value" in descriptor) || !descriptor.enumerable) fault("invalid_agent_record");
    result[key] = descriptor.value;
  }
  return result;
}

function proposal(value: unknown): WorkflowPlan {
  const plan = fields(value);
  const expected = ["summary", "source", "arguments", "task"];
  if (["summary", "source", "arguments"].some((key) => !(key in plan)) ||
      Object.keys(plan).some((key) => !expected.includes(key))) fault("invalid_agent_record");
  const identifier = (value: unknown): value is string => typeof value === "string"
    && value.length <= 64 && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(value)
    && !["fn", "true", "false", "none", "body"].includes(value) && !value.startsWith("_lese_");
  if ("task" in plan && !identifier(plan.task)) fault("invalid_plan_task");
  const arguments_ = fields(plan.arguments);
  const entries = Object.entries(arguments_);
  if (entries.length > 1 || entries.some(([name]) => !identifier(name))) fault("invalid_plan_arguments");
  const owned: Record<string, string> = Object.create(null);
  for (const [name, value] of entries) owned[name] = boundedText(value, 4096, "plan_text");
  return Object.freeze({
    summary: boundedText(plan.summary, 1024, "plan_summary", true),
    source: boundedText(plan.source, 16 * 1024, "plan_source", true),
    ...("task" in plan ? { task: plan.task as string } : {}),
    arguments: Object.freeze(owned),
  });
}

/** Own and freeze bounded JSON, rejecting coercions, accessors and cycles. */
function json(value: unknown, label: string, limit = JSON_LIMIT): AgentJson {
  // Raw JSON is already encoded. Preserve its exact numeric tokens and count
  // the wire bytes, not an additional JSON string wrapper's escape overhead.
  if (typeof value === "string") return boundedText(value, limit, label);
  let nodes = 0;
  let textBytes = 0;
  const seen = new Set<object>();
  function string(value: unknown): string {
    const checked = boundedText(value, limit, label);
    textBytes += encoder.encode(checked).length;
    if (textBytes > limit) fault(`oversized_${label}`);
    return checked;
  }
  function copy(value: unknown, depth: number): AgentJson {
    if (++nodes > JSON_NODE_LIMIT || depth > 32) fault(`oversized_${label}`);
    if (value === null || typeof value === "boolean") return value;
    if (typeof value === "number" && Number.isFinite(value)) return value;
    if (typeof value === "string") return string(value);
    if (typeof value !== "object" || value === null || seen.has(value)) fault(`invalid_${label}`);
    seen.add(value);
    try {
      if (Array.isArray(value)) {
        if (value.length > JSON_NODE_LIMIT) fault(`oversized_${label}`);
        const entries: AgentJson[] = [];
        // Reject holes, getters and extra properties instead of coercing them.
        if (Reflect.ownKeys(value).length !== value.length + 1) fault(`invalid_${label}`);
        for (let index = 0; index < value.length; index += 1) {
          const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
          if (!descriptor || !("value" in descriptor) || !descriptor.enumerable) fault(`invalid_${label}`);
          entries.push(copy(descriptor.value, depth + 1));
        }
        return Object.freeze(entries);
      }
      const record = fields(value);
      const result: Record<string, AgentJson> = Object.create(null);
      for (const [key, entry] of Object.entries(record)) result[string(key)] = copy(entry, depth + 1);
      return Object.freeze(result);
    } finally {
      seen.delete(value);
    }
  }
  const result = copy(value, 0);
  if (encoder.encode(JSON.stringify(result)).length > limit) fault(`oversized_${label}`);
  return result;
}

function errorText(error: unknown): string {
  try {
    const message = error instanceof Error ? error.message : typeof error === "string" ? error : "agent_task_failed";
    // Only display a bounded valid string; never stringify arbitrary thrown data.
    return boundedText(message, 1024, "error") || "agent_task_failed";
  } catch { return "agent_task_failed"; }
}

/**
 * One explicit user submission at a time. Cancellation does not release the
 * slot until outstanding provider/host work settles; late work cannot overlap
 * a new task. The host is responsible for bounded waits and real cancellation.
 */
export class AgentSession {
  readonly #provider: AgentProvider;
  readonly #host: AgentHost;
  readonly #onEvent?: (state: AgentState) => void;
  #active: Task | null = null;
  #lastId = 0;
  #state: AgentState = Object.freeze({ taskId: 0, phase: "idle", possiblePartialEffects: false });

  constructor(options: AgentSessionOptions) {
    if (typeof options.provider?.plan !== "function" ||
        [options.host?.describe, options.host?.execute, options.host?.verify].some((fn) => typeof fn !== "function") ||
        (options.onEvent !== undefined && typeof options.onEvent !== "function")) fault("invalid_agent_adapter");
    this.#provider = options.provider;
    this.#host = options.host;
    this.#onEvent = options.onEvent;
  }

  get busy(): boolean { return this.#active !== null; }
  get state(): AgentState { return this.#state; }

  async submit(message: string): Promise<AgentOutcome> {
    if (this.#active) fault("agent_task_pending");
    const admitted = boundedText(message, 8192, "message", true);
    if (this.#lastId === Number.MAX_SAFE_INTEGER) fault("agent_task_id_exhausted");
    const task: Task = { id: ++this.#lastId, controller: new AbortController(), enteredExecute: false };
    this.#active = task;
    return this.#run(task, admitted);
  }

  cancel(): boolean {
    const task = this.#active;
    if (!task || task.controller.signal.aborted) return false;
    task.controller.abort();
    this.#emit(task, "cancelling");
    return true;
  }

  #check(task: Task): void {
    if (this.#active !== task || task.controller.signal.aborted) fault("agent_task_cancelled");
  }

  #emit(task: Task, phase: AgentPhase, error?: string): void {
    this.#state = Object.freeze({
      taskId: task.id, phase,
      possiblePartialEffects: phase === "verified" ? false : task.enteredExecute,
      ...(task.plan ? { summary: task.plan.summary } : {}),
      ...(error ? { error } : {}),
    });
    try { this.#onEvent?.(this.#state); } catch { /* Observers cannot grant or reject execution. */ }
  }

  async #run(task: Task, message: string): Promise<AgentOutcome> {
    const signal = task.controller.signal;
    let status: AgentOutcome["status"] = "failed";
    let error: string | undefined;
    try {
      this.#emit(task, "describing");
      this.#check(task);
      const described = await this.#host.describe(signal);
      this.#check(task);
      const catalog = json(described, "catalog", CATALOG_LIMIT);
      this.#check(task);
      this.#emit(task, "planning");
      this.#check(task);
      const planned = await this.#provider.plan(Object.freeze({ taskId: task.id, message, catalog }), signal);
      this.#check(task);
      task.plan = proposal(planned);
      this.#check(task);
      this.#emit(task, "executing");
      this.#check(task);
      task.enteredExecute = true;
      // Keep the public state accurate while awaiting the host without emitting
      // another phase transition. Admission observers can cancel before entry.
      this.#state = Object.freeze({ ...this.#state, possiblePartialEffects: true });
      const executed = await this.#host.execute(task.plan, signal);
      this.#check(task);
      task.report = json(executed, "report");
      this.#check(task);
      this.#emit(task, "verifying");
      this.#check(task);
      const verified = await this.#host.verify(task.report, task.plan, signal);
      this.#check(task);
      if (verified !== true) fault("application_verification_failed");
      status = "verified";
    } catch (failure) {
      status = signal.aborted ? "cancelled" : "failed";
      error = status === "cancelled" ? "agent_task_cancelled" : errorText(failure);
    }
    const outcome: AgentOutcome = Object.freeze({
      taskId: task.id, status,
      possiblePartialEffects: status !== "verified" && task.enteredExecute,
      ...(task.plan ? { plan: task.plan } : {}),
      ...(task.report !== undefined ? { report: task.report } : {}),
      ...(error ? { error } : {}),
    });
    this.#active = null;
    this.#emit(task, status, error);
    return outcome;
  }
}
