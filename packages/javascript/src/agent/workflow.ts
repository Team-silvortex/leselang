import {
  AgentSession, type AgentContext, type AgentHost, type AgentJson, type AgentOutcome,
  type AgentPhase, type AgentState, type WorkflowPlan,
} from "./session.js";
import { boundedText, errorText, fault, fields, json, proposal } from "./data.js";
import { prepareSemanticCatalog, type SemanticCatalog } from "../semantics/index.js";
import { settleVerificationResult, type VerificationDiagnostic } from "../verification.js";
import {
  admitWorkflowBindings, admitWorkflowRequirements, checkWorkflowRequirement, resolveWorkflowStep,
  type AgentWorkflowBinding, type AgentWorkflowRequirement, type AgentWorkflowRequirementOutcome,
} from "./workflow-bindings.js";

export { prepareSemanticWorkflow } from "./workflow-bindings.js";

/** An ordered, finite proposal. Every step must be a declared semantic task. */
export interface AgentWorkflowPlan {
  readonly steps: readonly WorkflowPlan[];
  readonly bindings?: readonly AgentWorkflowBinding[];
  readonly requirements?: readonly AgentWorkflowRequirement[];
}

export interface AgentWorkflowProvider {
  plan(context: AgentContext, signal: AbortSignal): Promise<AgentWorkflowPlan>;
}

export interface AgentWorkflowState extends AgentState {
  readonly totalSteps: number;
  readonly completedSteps: number;
  /** Zero-based index of the step being considered or executed. */
  readonly stepIndex?: number;
}

export interface AgentWorkflowOutcome {
  readonly taskId: number;
  readonly status: "verified" | "failed" | "cancelled";
  readonly possiblePartialEffects: boolean;
  readonly plan?: AgentWorkflowPlan;
  /** Attempted steps, including a failed/cancelled step. Unstarted steps are absent. */
  readonly steps: readonly AgentOutcome[];
  /** Only comparisons that actually completed; a match does not imply execution. */
  readonly requirements?: readonly AgentWorkflowRequirementOutcome[];
  readonly error?: string;
  readonly verificationFailure?: VerificationDiagnostic;
  /** Zero-based attempted or next step where progress stopped. */
  readonly stoppedStep?: number;
}

export interface AgentWorkflowSessionOptions {
  readonly provider: AgentWorkflowProvider;
  readonly host: AgentHost;
  readonly onEvent?: (state: AgentWorkflowState) => void;
}

interface Task {
  readonly id: number;
  readonly controller: AbortController;
  readonly outcomes: AgentOutcome[];
  readonly requirements: AgentWorkflowRequirementOutcome[];
  enteredExecute: boolean;
  completed: number;
  stepIndex?: number;
  plan?: AgentWorkflowPlan;
  child?: AgentSession;
}

const CATALOG_LIMIT = 256 * 1024;
const MAX_STEPS = 8;

function admit(value: unknown, catalog: SemanticCatalog, rawCatalog: unknown): AgentWorkflowPlan {
  const candidate = fields(value);
  if (!("steps" in candidate) || Object.keys(candidate).some((key) => !["steps", "bindings", "requirements"].includes(key))) {
    fault("invalid_workflow_steps");
  }
  const values = candidate.steps;
  if (!Array.isArray(values) || values.length === 0 || values.length > MAX_STEPS
    || Reflect.ownKeys(values).length !== values.length + 1) fault("invalid_workflow_steps");
  const steps: WorkflowPlan[] = [];
  for (let index = 0; index < values.length; index++) {
    const descriptor = Object.getOwnPropertyDescriptor(values, String(index));
    if (!descriptor?.enumerable || !("value" in descriptor)) fault("invalid_workflow_steps");
    const step = proposal(descriptor.value);
    if (step.task === undefined) fault("invalid_workflow_task");
    if (Object.values(step.arguments).some((value) => value.includes("\0"))) fault("invalid_workflow_arguments");
    const prepared = catalog.prepare(step.task, step.arguments).plan;
    if (step.source !== prepared.source) fault("invalid_workflow_source");
    if (step.inputMode !== prepared.inputMode) fault("invalid_workflow_input_mode");
    // The full plan is owned before the first await that can enter execution.
    // Summaries and source are always taken from the host declaration.
    steps.push(prepared);
  }
  const bindings = "bindings" in candidate ? admitWorkflowBindings(candidate.bindings, steps, catalog, rawCatalog) : [];
  const requirements = "requirements" in candidate ? admitWorkflowRequirements(candidate.requirements, steps, rawCatalog) : [];
  return Object.freeze({ steps: Object.freeze(steps), ...(bindings.length ? { bindings } : {}),
    ...(requirements.length ? { requirements } : {}) });
}

/**
 * Sequential composition of existing AgentSession calls, with no VM scheduler.
 * All 1–8 steps are admitted before execution. Each step reads a fresh catalog
 * and requires the same owned snapshot: raw JSON is compared byte-for-byte;
 * object catalogs use their JSON representation, including property order.
 */
export class AgentWorkflowSession {
  readonly #provider: AgentWorkflowProvider;
  readonly #host: AgentHost;
  readonly #onEvent?: (state: AgentWorkflowState) => void;
  #active: Task | null = null;
  #lastId = 0;
  #state: AgentWorkflowState = Object.freeze({
    taskId: 0, phase: "idle", possiblePartialEffects: false, totalSteps: 0, completedSteps: 0,
  });

  constructor(options: AgentWorkflowSessionOptions) {
    if (typeof options.provider?.plan !== "function"
      || [options.host?.describe, options.host?.execute, options.host?.verify].some((fn) => typeof fn !== "function")
      || (options.host?.verifyDetailed !== undefined && typeof options.host.verifyDetailed !== "function")
      || (options.onEvent !== undefined && typeof options.onEvent !== "function")) fault("invalid_agent_adapter");
    this.#provider = options.provider;
    this.#host = options.host;
    this.#onEvent = options.onEvent;
  }

  get busy(): boolean { return this.#active !== null; }
  get state(): AgentWorkflowState { return this.#state; }

  async submit(message: string): Promise<AgentWorkflowOutcome> {
    if (this.#active) fault("agent_task_pending");
    const admitted = boundedText(message, 8192, "message", true);
    if (this.#lastId === Number.MAX_SAFE_INTEGER) fault("agent_task_id_exhausted");
    const task: Task = {
      id: ++this.#lastId, controller: new AbortController(), outcomes: [], requirements: [], enteredExecute: false, completed: 0,
    };
    this.#active = task;
    return this.#run(task, admitted);
  }

  cancel(): boolean {
    const task = this.#active;
    if (!task || task.controller.signal.aborted) return false;
    task.controller.abort();
    task.child?.cancel();
    if (this.#state.phase !== "cancelling") this.#emit(task, "cancelling");
    return true;
  }

  #check(task: Task): void {
    if (this.#active !== task || task.controller.signal.aborted) fault("agent_task_cancelled");
  }

  #emit(task: Task, phase: AgentPhase, error?: string, verificationFailure?: VerificationDiagnostic): void {
    const step = task.stepIndex === undefined ? undefined : task.plan?.steps[task.stepIndex];
    this.#state = Object.freeze({
      taskId: task.id, phase,
      possiblePartialEffects: phase === "verified" ? false : task.enteredExecute,
      totalSteps: task.plan?.steps.length ?? 0, completedSteps: task.completed,
      ...(task.stepIndex !== undefined ? { stepIndex: task.stepIndex } : {}),
      ...(step ? { summary: step.summary } : {}),
      ...(error ? { error } : {}),
      ...(phase === "failed" && verificationFailure ? { verificationFailure } : {}),
    });
    try { this.#onEvent?.(this.#state); } catch { /* Display observers do not authorize work. */ }
  }

  async #run(task: Task, message: string): Promise<AgentWorkflowOutcome> {
    const signal = task.controller.signal;
    let status: AgentWorkflowOutcome["status"] = "failed";
    let error: string | undefined;
    let verificationFailure: VerificationDiagnostic | undefined;
    try {
      this.#emit(task, "describing");
      this.#check(task);
      const described = await this.#host.describe(signal);
      this.#check(task);
      const owned = json(described, "catalog", CATALOG_LIMIT);
      const snapshot = JSON.stringify(owned);
      const catalog = prepareSemanticCatalog(owned);
      this.#emit(task, "planning");
      this.#check(task);
      const planned = await this.#provider.plan(Object.freeze({ taskId: task.id, message, catalog: owned }), signal);
      this.#check(task);
      task.plan = admit(planned, catalog, owned);
      this.#check(task);
      const referenced = new Set([
        ...(task.plan.bindings?.map((binding) => binding.fromStep) ?? []),
        ...(task.plan.requirements?.map((requirement) => requirement.fromStep) ?? []),
      ]);

      task.child = new AgentSession({
        provider: { plan: async () => {
          this.#check(task);
          const requirement = checkWorkflowRequirement(task.plan!, task.stepIndex!, task.outcomes);
          if (requirement) {
            task.requirements.push(requirement);
            if (!requirement.satisfied) fault("workflow_requirement_not_met");
          }
          const concrete = resolveWorkflowStep(task.plan!, task.stepIndex!, task.outcomes, catalog);
          this.#check(task);
          return concrete;
        } },
        host: {
          describe: async (childSignal): Promise<AgentJson> => {
            this.#check(task);
            const fresh = await this.#host.describe(childSignal);
            this.#check(task);
            const current = json(fresh, "catalog", CATALOG_LIMIT);
            if (JSON.stringify(current) !== snapshot) fault("workflow_catalog_changed");
            return current;
          },
          execute: async (plan, childSignal) => {
            this.#check(task);
            task.enteredExecute = true;
            this.#state = Object.freeze({ ...this.#state, possiblePartialEffects: true });
            return this.#host.execute(plan, childSignal);
          },
          verify: async (report, plan, childSignal) => {
            this.#check(task);
            if (referenced.has(task.stepIndex!) && !catalog.prepare(plan.task!, plan.arguments).verify(report)) return false;
            this.#check(task);
            const verified = await this.#host.verify(report, plan, childSignal);
            this.#check(task);
            return verified;
          },
          ...(this.#host.verifyDetailed ? {
            verifyDetailed: async (report: AgentJson, plan: WorkflowPlan, childSignal: AbortSignal) => {
              this.#check(task);
              if (referenced.has(task.stepIndex!)) {
                const checked = catalog.prepare(plan.task!, plan.arguments).verifyDetailed(report);
                if (!checked.verified) return checked;
              }
              this.#check(task);
              const checked = await settleVerificationResult(this.#host.verifyDetailed!(report, plan, childSignal));
              this.#check(task);
              return checked;
            },
          } : {}),
        },
        onEvent: (state) => {
          // A verified step is not yet a verified workflow. Terminal child
          // states are published only after their outcome has been retained.
          if (!["verified", "failed", "cancelled"].includes(state.phase)) this.#emit(task, state.phase, state.error);
        },
      });
      for (let index = 0; index < task.plan.steps.length; index++) {
        task.stepIndex = index;
        this.#check(task);
        const outcome = await task.child.submit(message);
        task.outcomes.push(outcome);
        if (outcome.status === "verified") task.completed += 1;
        else {
          status = outcome.status;
          error = outcome.error;
          verificationFailure = outcome.verificationFailure;
          break;
        }
        this.#check(task);
        if (index + 1 < task.plan.steps.length) {
          // Expose the completed count before admitting the next step. A UI
          // observer may cancel here and prevent every subsequent operation.
          this.#emit(task, "verifying");
          task.stepIndex = index + 1;
          this.#check(task);
        } else status = "verified";
      }
    } catch (failure) {
      status = signal.aborted ? "cancelled" : "failed";
      error = status === "cancelled" ? "agent_task_cancelled" : errorText(failure);
    }
    if (signal.aborted) {
      status = "cancelled";
      error = "agent_task_cancelled";
    }
    const outcome: AgentWorkflowOutcome = Object.freeze({
      taskId: task.id, status,
      possiblePartialEffects: status !== "verified" && task.enteredExecute,
      ...(task.plan ? { plan: task.plan } : {}),
      steps: Object.freeze([...task.outcomes]),
      ...(task.requirements.length ? { requirements: Object.freeze([...task.requirements]) } : {}),
      ...(error ? { error } : {}),
      ...(status === "failed" && verificationFailure ? { verificationFailure } : {}),
      ...(status !== "verified" && task.stepIndex !== undefined ? { stoppedStep: task.stepIndex } : {}),
    });
    task.child = undefined;
    this.#active = null;
    this.#emit(task, status, error, verificationFailure);
    return outcome;
  }
}
