import type { AgentOutcome, WorkflowPlan } from "./session.js";
import { boundedText, fault, fields } from "./data.js";
import { prepareSemanticCatalog, type SemanticCatalog } from "../semantics/index.js";
import { validate, verificationResultType } from "../semantics/catalog.js";
import { bool, decode, record, text, uint } from "../semantics/json.js";
import type { AgentWorkflowPlan } from "./workflow.js";

export type WorkflowResultFormat = "string" | "decimal" | "boolean";

/** A complete value from an earlier verified step, with explicit conversion. */
export interface WorkflowResultReference {
  readonly fromStep: number;
  readonly format: WorkflowResultFormat;
}

/** Continue only when an earlier verified result equals this exact text. */
export interface WorkflowResultRequirement extends WorkflowResultReference {
  readonly equals: string;
}

export interface AgentWorkflowRequirement extends WorkflowResultRequirement {
  readonly step: number;
}

/** A completed comparison, not evidence that its target step executed. */
export interface AgentWorkflowRequirementOutcome extends AgentWorkflowRequirement {
  readonly actual: string;
  readonly satisfied: boolean;
}

export interface SemanticWorkflowStep {
  readonly task: string;
  readonly arguments: Readonly<Record<string, string | WorkflowResultReference>>;
  readonly requires?: WorkflowResultRequirement;
}

/** Zero-based indices. The target template argument must be the empty string. */
export interface AgentWorkflowBinding extends WorkflowResultReference {
  readonly step: number;
  readonly argument: string;
}

const MAX_STEPS = 8;
const CATALOG_LIMIT = 256 * 1024;

/** Own a finite dense list without invoking accessors or iterators. */
function entries(value: unknown, nonempty: boolean, label = "bindings"): unknown[] {
  if (!Array.isArray(value) || value.length > MAX_STEPS || (nonempty && value.length === 0)
    || Reflect.ownKeys(value).length !== value.length + 1) fault(`invalid_workflow_${label}`);
  const result: unknown[] = [];
  for (let index = 0; index < value.length; index++) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    if (!descriptor?.enumerable || !("value" in descriptor)) fault(`invalid_workflow_${label}`);
    result.push(descriptor.value);
  }
  return result;
}

function format(value: unknown, label = "binding"): WorkflowResultFormat {
  if (value !== "string" && value !== "decimal" && value !== "boolean") fault(`invalid_workflow_${label}_format`);
  return value;
}

function index(value: unknown, label = "binding"): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value >= MAX_STEPS) {
    fault(`invalid_workflow_${label}_index`);
  }
  return value;
}

/** Used by both the public helper and admission of a provider's full proposal. */
export function admitWorkflowBindings(
  value: unknown, steps: readonly WorkflowPlan[], catalog: SemanticCatalog, rawCatalog: unknown,
): readonly AgentWorkflowBinding[] {
  const values = entries(value, false);
  if (values.length === 0) return Object.freeze([]);
  // The result type is the host's declared report contract. Public task info
  // remains compatible; no model-selected type or conversion is inferred.
  const declared = validate(decode(rawCatalog, CATALOG_LIMIT));
  const declarations = declared.tasks;
  const used = new Set<string>();
  return Object.freeze(values.map((value) => {
    const binding = fields(value, ["step", "argument", "fromStep", "format"]);
    const step = index(binding.step), fromStep = index(binding.fromStep);
    const argument = boundedText(binding.argument, 64, "workflow_binding_argument", true);
    const conversion = format(binding.format);
    const source = steps[fromStep], target = steps[step];
    if (!source || !target || fromStep >= step) fault("invalid_workflow_binding_reference");
    const input = catalog.tasks.find((task) => task.id === target.task)?.inputName;
    if (input !== argument || target.arguments[argument] !== "") fault("invalid_workflow_binding_target");
    const key = `${step}:${argument}`;
    if (used.has(key)) fault("duplicate_workflow_binding");
    used.add(key);
    const declaration = declarations.get(source.task!);
    const kind = verificationResultType(record(declaration?.verification).result, declared.semanticVersion);
    if (kind !== (conversion === "decimal" ? "integer" : conversion)) fault("invalid_workflow_binding_format");
    return Object.freeze({ step, argument, fromStep, format: conversion });
  }));
}

/** Admit every condition before any step is allowed to execute. */
export function admitWorkflowRequirements(
  value: unknown, steps: readonly WorkflowPlan[], rawCatalog: unknown,
): readonly AgentWorkflowRequirement[] {
  const values = entries(value, false, "requirements");
  if (values.length === 0) return Object.freeze([]);
  const declared = validate(decode(rawCatalog, CATALOG_LIMIT));
  const used = new Set<number>();
  return Object.freeze(values.map((value) => {
    const requirement = fields(value, ["step", "fromStep", "format", "equals"]);
    const step = index(requirement.step, "requirement"), fromStep = index(requirement.fromStep, "requirement");
    const conversion = format(requirement.format, "requirement");
    const source = steps[fromStep];
    if (!source || !steps[step] || fromStep >= step) fault("invalid_workflow_requirement_reference");
    if (used.has(step)) fault("duplicate_workflow_requirement");
    used.add(step);
    const declaration = declared.tasks.get(source.task!);
    const kind = verificationResultType(record(declaration?.verification).result, declared.semanticVersion);
    if (kind !== (conversion === "decimal" ? "integer" : conversion)) fault("invalid_workflow_requirement_format");
    const equals = boundedText(requirement.equals, 4096, "workflow_requirement_equals");
    if (equals.includes("\0")
      || (conversion === "decimal" && (!/^(0|[1-9][0-9]{0,19})$/u.test(equals) || BigInt(equals) > 18_446_744_073_709_551_615n))
      || (conversion === "boolean" && equals !== "true" && equals !== "false")) fault("invalid_workflow_requirement_equals");
    return Object.freeze({ step, fromStep, format: conversion, equals });
  }));
}

/**
 * Prepare a complete finite workflow from literal inputs or backward result
 * references. No host work occurs; references become empty string placeholders
 * plus separately admitted bindings, never source interpolation.
 */
export function prepareSemanticWorkflow(catalogValue: unknown, values: readonly SemanticWorkflowStep[]): AgentWorkflowPlan {
  const catalog = prepareSemanticCatalog(catalogValue);
  const bindings: AgentWorkflowBinding[] = [];
  const requirements: AgentWorkflowRequirement[] = [];
  const steps = entries(values, true).map((value, step) => {
    const candidate = fields(value);
    if (!("task" in candidate) || !("arguments" in candidate)
      || Object.keys(candidate).some((key) => !["task", "arguments", "requires"].includes(key))) fault("invalid_agent_record");
    const task = boundedText(candidate.task, 64, "workflow_task", true);
    if ("requires" in candidate) {
      const requirement = fields(candidate.requires, ["fromStep", "format", "equals"]);
      requirements.push({ step, fromStep: index(requirement.fromStep, "requirement"), format: format(requirement.format, "requirement"),
        equals: boundedText(requirement.equals, 4096, "workflow_requirement_equals") });
    }
    if (typeof candidate.arguments === "object" && candidate.arguments !== null
      && Reflect.ownKeys(candidate.arguments).length > 1) fault("invalid_workflow_arguments");
    const supplied = fields(candidate.arguments);
    const arguments_: Record<string, string> = Object.create(null);
    for (const [argument, suppliedValue] of Object.entries(supplied)) {
      boundedText(argument, 64, "workflow_binding_argument", true);
      if (typeof suppliedValue === "string") {
        const literal = boundedText(suppliedValue, 4096, "workflow_argument");
        if (literal.includes("\0")) fault("invalid_workflow_arguments");
        arguments_[argument] = literal;
      } else {
        const reference = fields(suppliedValue, ["fromStep", "format"]);
        bindings.push({ step, argument, fromStep: index(reference.fromStep), format: format(reference.format) });
        arguments_[argument] = "";
      }
    }
    return catalog.prepare(task, arguments_).plan;
  });
  const admitted = admitWorkflowBindings(bindings, steps, catalog, catalogValue);
  const required = admitWorkflowRequirements(requirements, steps, catalogValue);
  return Object.freeze({ steps: Object.freeze(steps), ...(admitted.length ? { bindings: admitted } : {}),
    ...(required.length ? { requirements: required } : {}) });
}

/** Extract only an actual verified report result, retaining exact u64 digits. */
function resultText(outcome: AgentOutcome | undefined, conversion: WorkflowResultFormat): string {
  if (!outcome || outcome.status !== "verified" || outcome.report === undefined) fault("workflow_result_unverified");
  try {
    const result = record(record(decode(outcome.report, 128 * 1024)).result, ["type", "value"]);
    const expected = conversion === "decimal" ? "integer" : conversion;
    if (result.type !== expected) throw new Error();
    const value = conversion === "decimal" ? uint(result.value).toString()
      : conversion === "boolean" ? String(bool(result.value)) : text(result.value, 4096);
    if (value.includes("\0")) throw new Error();
    return value;
  } catch { return fault("invalid_workflow_result"); }
}

/** Compare actual verified evidence; declarations never supply the actual value. */
export function checkWorkflowRequirement(
  plan: AgentWorkflowPlan, step: number, outcomes: readonly AgentOutcome[],
): AgentWorkflowRequirementOutcome | undefined {
  const requirement = plan.requirements?.find((requirement) => requirement.step === step);
  if (!requirement) return undefined;
  const actual = resultText(outcomes[requirement.fromStep], requirement.format);
  return Object.freeze({ ...requirement, actual, satisfied: actual === requirement.equals });
}

/** Called only after earlier child outcomes have passed both required verifiers. */
export function resolveWorkflowStep(
  plan: AgentWorkflowPlan, step: number, outcomes: readonly AgentOutcome[], catalog: SemanticCatalog,
): WorkflowPlan {
  const template = plan.steps[step]!;
  const bindings = plan.bindings?.filter((binding) => binding.step === step) ?? [];
  if (bindings.length === 0) return template;
  const arguments_: Record<string, string> = Object.assign(Object.create(null), template.arguments);
  for (const binding of bindings) arguments_[binding.argument] = resultText(outcomes[binding.fromStep], binding.format);
  return catalog.prepare(template.task!, arguments_).plan;
}
