/** Host-authored, bounded semantic tasks. This module never executes an Effect. */
import type { WorkflowPlan } from "./agent.js";
import { taskInput, validate } from "./semantics/catalog.js";
import { decode, fail, list, record, text } from "./semantics/json.js";
import { generate } from "./semantics/source.js";
import { verify } from "./semantics/report.js";

export interface SemanticTaskInfo {
  readonly id: string;
  readonly summary: string;
  readonly inputName?: string;
}

export interface PreparedSemanticTask {
  readonly plan: WorkflowPlan;
  readonly expectedEffects: number;
  readonly checks: readonly string[];
  /** Check host evidence against the admitted task, independently of provider claims. */
  verify(report: unknown): boolean;
}

export interface SemanticCatalog {
  readonly profile: string;
  readonly tasks: readonly SemanticTaskInfo[];
  prepare(task: string, arguments_: Readonly<Record<string, string>>): PreparedSemanticTask;
}

/**
 * Validate semantics v1 without a profile-specific strategy or runtime action.
 * Raw JSON preserves duplicate-key and u64 checks; decoded objects must contain
 * only exact safe Number integers. Returned plans contain strings, never BigInt.
 */
export function prepareSemanticCatalog(value: unknown): SemanticCatalog {
  const catalog = validate(decode(value, 256 * 1024));
  const tasks = Object.freeze([...catalog.tasks.values()].map((task): SemanticTaskInfo => {
    const inputName = taskInput(task);
    return Object.freeze({ id: task.id as string, summary: task.summary as string, ...(inputName ? { inputName } : {}) });
  }));
  return Object.freeze({
    profile: catalog.profile, tasks,
    prepare(taskId: string, arguments_: Readonly<Record<string, string>>): PreparedSemanticTask {
      const task = catalog.tasks.get(taskId);
      if (!task) fail("undeclared task");
      const input = taskInput(task), raw = record(decode(arguments_, 16 * 1024), input ? [input] : []);
      const args: Record<string, string> = Object.create(null);
      if (input) args[input] = text(raw[input], 4096);
      Object.freeze(args);
      const source = generate(catalog, task);
      const plan: WorkflowPlan = Object.freeze({ task: taskId, summary: task.summary as string, source, arguments: args });
      const expectedEffects = list(task.steps, 21).filter((step) => record(step).kind === "call").length;
      const checks = Object.freeze(list(record(task.verification).predicates, 64).map((predicate) => record(predicate).id as string));
      return Object.freeze({ plan, expectedEffects, checks,
        verify: (report: unknown) => verify(catalog, task, args, expectedEffects, report) });
    },
  });
}
