import { boundedText, errorText, fault, json, proposal } from "./agent-data.js";

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

const CATALOG_LIMIT = 256 * 1024;

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
