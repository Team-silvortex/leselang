/** Host-authored, bounded semantic tasks. This module never executes an Effect. */
import type { WorkflowPlan } from "../agent/session.js";
import { taskInputs, validate } from "./catalog.js";
import { decode, encoder, fail, list, record, text } from "./json.js";
import { generate } from "./source.js";
import { verifyDetailed } from "./report.js";
import type { VerificationResult } from "../verification.js";

export interface SemanticTaskInfo {
  readonly id: string;
  readonly summary: string;
  readonly inputName?: string;
  /** Present only for v3, including the empty list of a no-input task. */
  readonly inputNames?: readonly string[];
}

export interface PreparedSemanticTask {
  readonly plan: WorkflowPlan;
  readonly expectedEffects: number;
  readonly checks: readonly string[];
  /** Check host evidence against the admitted task, independently of provider claims. */
  verify(report: unknown): boolean;
  /** Identify the earliest failed check without exposing report values. */
  verifyDetailed(report: unknown): VerificationResult;
}

export interface SemanticCatalog {
  readonly profile: string;
  readonly tasks: readonly SemanticTaskInfo[];
  prepare(task: string, arguments_: Readonly<Record<string, string>>): PreparedSemanticTask;
}

/**
 * Validate semantics v1/v2/v3 without a profile-specific strategy or runtime action.
 * Raw JSON preserves duplicate-key and u64 checks; decoded objects must contain
 * only exact safe Number integers. Returned plans contain strings, never BigInt.
 */
export function prepareSemanticCatalog(value: unknown): SemanticCatalog {
  const catalog = validate(decode(value, 256 * 1024));
  const tasks = Object.freeze([...catalog.tasks.values()].map((task): SemanticTaskInfo => {
    const inputs = taskInputs(task, catalog.semanticVersion);
    return Object.freeze({ id: task.id as string, summary: task.summary as string,
      ...(catalog.semanticVersion === 3n ? { inputNames: Object.freeze(inputs) } : inputs.length ? { inputName: inputs[0]! } : {}) });
  }));
  return Object.freeze({
    profile: catalog.profile, tasks,
    prepare(taskId: string, arguments_: Readonly<Record<string, string>>): PreparedSemanticTask {
      const task = catalog.tasks.get(taskId);
      if (!task) fail("undeclared task");
      const inputs = taskInputs(task, catalog.semanticVersion);
      const raw = record(decode(arguments_, catalog.semanticVersion === 3n ? 128 * 1024 : 16 * 1024), inputs);
      const args: Record<string, string> = Object.create(null);
      let bytes = 0;
      for (const input of inputs) {
        args[input] = text(raw[input], 4096);
        bytes += encoder.encode(args[input]).length;
        if (catalog.semanticVersion === 3n && args[input].includes("\0")) fail("task input contains NUL");
      }
      if (catalog.semanticVersion === 3n && bytes > 16 * 1024) fail("task argument total bound");
      Object.freeze(args);
      const source = generate(catalog, task);
      const plan: WorkflowPlan = Object.freeze({ task: taskId, summary: task.summary as string, source, arguments: args,
        ...(catalog.semanticVersion === 3n ? { inputMode: "named_strings" as const } : {}) });
      const expectedEffects = list(task.steps, 21).filter((step) => record(step).kind === "call").length;
      const checks = Object.freeze(list(record(task.verification).predicates, 64).map((predicate) => record(predicate).id as string));
      return Object.freeze({ plan, expectedEffects, checks,
        verify: (report: unknown) => verifyDetailed(catalog, task, args, expectedEffects, report).verified,
        verifyDetailed: (report: unknown) => verifyDetailed(catalog, task, args, expectedEffects, report) });
    },
  });
}
