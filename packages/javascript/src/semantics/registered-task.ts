/** Expand registered-control steps into the existing finite semantic vocabulary. */
import { verificationResultType } from "./catalog.js";
import type {
  SemanticLiteral, SemanticTaskDescriptionV2, SemanticTaskDescriptionV3, SemanticTaskStep, SemanticTerm,
} from "./declarations.js";
import { bool, fail, list, ownDeclaration, record, require_, text, uint, type Json, type RecordValue } from "./json.js";

export interface RegisteredReadControlStep {
  readonly kind: "read_control";
  readonly control: string;
  /** Public binding for the current snapshot. */
  readonly binding: string;
}

export interface RegisteredSetControlTextStep {
  readonly kind: "set_control_text";
  readonly control: string;
  /** Public binding for the snapshot observed after submission. */
  readonly binding: string;
  /** Optional public binding for the snapshot used to admit the action. */
  readonly before?: string;
  readonly text: SemanticTerm;
}

export interface RegisteredClickControlStep {
  readonly kind: "click_control";
  readonly control: string;
  readonly binding: string;
  readonly before?: string;
}

export type RegisteredTaskStep = SemanticTaskStep | RegisteredReadControlStep |
  RegisteredSetControlTextStep | RegisteredClickControlStep;

export interface RegisteredTaskDefinition extends Omit<SemanticTaskDescriptionV2, "steps"> {
  readonly steps: readonly RegisteredTaskStep[];
  /** Every generated guard returns this explicitly selected scalar on failure. */
  readonly failure: SemanticLiteral;
}

/** Explicit v3 declaration; the original constructor remains v2-only. */
export interface RegisteredTaskDefinitionV3 extends Omit<SemanticTaskDescriptionV3, "steps"> {
  readonly steps: readonly RegisteredTaskStep[];
  readonly failure: SemanticLiteral;
}

const LIMIT = 256 * 1024;
const RESERVED = new Set(["fn", "true", "false", "none", "body"]);

function name(value: Json | undefined): string {
  const result = text(value, 64, true);
  require_(/^[A-Za-z_][A-Za-z_0-9]*$/u.test(result) && !RESERVED.has(result)
    && !result.startsWith("_lese_"), "registered task identifier");
  return result;
}

function literal(value: Json | undefined): string {
  const data = record(value);
  record(data, data.kind === "none" ? ["kind"] : ["kind", "value"]);
  if (data.kind === "string") text(data.value, 4096);
  else if (data.kind === "integer") uint(data.value);
  else if (data.kind === "boolean") bool(data.value);
  else require_(data.kind === "none", "registered task failure literal");
  return data.kind as string;
}

function term(value: Json | undefined): void {
  const data = record(value);
  switch (data.kind) {
    case "literal": record(data, ["kind", "value"]); literal(data.value); break;
    case "input": case "binding": record(data, ["kind", "name"]); name(data.name); break;
    case "field": record(data, ["kind", "binding", "name"]); name(data.binding); text(data.name, 128, true); break;
    default: fail("registered task term");
  }
}

function condition(value: Json): void {
  const data = record(value);
  switch (data.kind) {
    case "equal": record(data, ["kind", "left", "right"]); term(data.left); term(data.right); break;
    case "contains": record(data, ["kind", "list", "value"]); term(data.list); term(data.value); break;
    case "increased_by":
      record(data, ["kind", "before", "after", "amount"]); term(data.before); term(data.after); uint(data.amount); break;
    case "optional_equals": record(data, ["kind", "optional", "value"]); term(data.optional); term(data.value); break;
    default: fail("registered task condition");
  }
}

function freeze(value: Json): Json {
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}

/**
 * Build an owned, frozen v2 task declaration, without executing or granting an
 * Effect. Control steps expand to ordinary calls and guards. Pass the result
 * to defineSemanticCatalog with the actual native catalog to validate operation
 * signatures, references and report predicates before planning or execution.
 * Business success remains the application's explicit result and verification.
 */
export function defineRegisteredTask(options: RegisteredTaskDefinition): SemanticTaskDescriptionV2 {
  return expandRegisteredTask(options, 2n) as SemanticTaskDescriptionV2;
}

/** Expand v3 named-string tasks, then validate them against the real catalog. */
export function defineRegisteredTaskV3(options: RegisteredTaskDefinitionV3): SemanticTaskDescriptionV3 {
  return expandRegisteredTask(options, 3n) as SemanticTaskDescriptionV3;
}

function expandRegisteredTask(
  options: RegisteredTaskDefinition | RegisteredTaskDefinitionV3, version: 2n | 3n,
): SemanticTaskDescriptionV2 | SemanticTaskDescriptionV3 {
  const data = record(ownDeclaration(options, LIMIT),
    ["id", "summary", "inputs", "steps", "result", "execution", "verification", "failure"]);
  name(data.id); text(data.summary, 1024, true);
  const inputs = list(data.inputs, version === 3n ? 8 : 1);
  const declared = new Set<string>();
  const declare = (value: Json | undefined): string => {
    const result = name(value);
    require_(!declared.has(result), "duplicate registered task binding");
    declared.add(result);
    return result;
  };
  for (const raw of inputs) {
    const input = record(raw, ["name", "scalar_type"]);
    require_(input.scalar_type === "string", "registered task input must be string");
    declare(input.name);
  }
  require_((version === 3n ? ["named_strings", "no_text"] : ["named_text", "text", "no_text"])
    .includes(text(data.execution, 16)), "registered task execution style");
  require_((data.execution === "no_text") === (inputs.length === 0), "registered task input convention");
  term(data.result);
  const verification = record(data.verification,
    ["kind", "max_fuel", "result", "require_verified", "predicates"]);
  text(verification.kind, 128, true);
  require_(uint(verification.max_fuel, 1_000_000n) > 0n, "registered task Fuel ceiling");
  bool(verification.require_verified);
  const resultType = verificationResultType(verification.result, version);
  require_(literal(data.failure) === resultType, "registered task failure result type");
  const predicateIds = new Set<string>();
  for (const value of list(verification.predicates, 64, true)) {
    const predicate = record(value, ["id", "left", "relation", "right"]);
    const id = name(predicate.id);
    require_(!predicateIds.has(id), "duplicate registered task predicate");
    predicateIds.add(id);
  }

  const steps = list(data.steps, 21, true).map((value) => record(value));
  for (const step of steps) {
    if (step.kind === "call") {
      record(step, ["kind", "binding", "operation", "arguments"]);
      declare(step.binding); text(step.operation, 128, true);
      const parameters = new Set<string>();
      for (const value of list(step.arguments, 64)) {
        const argument = record(value, ["name", "value"]);
        const parameter = text(argument.name, 64, true);
        require_(!parameters.has(parameter), "duplicate registered task argument");
        parameters.add(parameter); term(argument.value);
      }
    } else if (step.kind === "guard") {
      record(step, ["kind", "conditions", "failure"]);
      require_(literal(step.failure) === resultType, "registered task guard result type");
      for (const value of list(step.conditions, 8, true)) condition(value);
    } else if (["read_control", "set_control_text", "click_control"].includes(step.kind as string)) {
      const fields = ["kind", "control", "binding"];
      if (step.kind !== "read_control" && Object.hasOwn(step, "before")) fields.push("before");
      if (step.kind === "set_control_text") fields.push("text");
      record(step, fields);
      require_(/^[A-Za-z0-9_-]{1,64}$/u.test(text(step.control, 64, true)), "registered control identifier");
      declare(step.binding);
      if (Object.hasOwn(step, "before")) declare(step.before);
      if (step.kind === "set_control_text") term(step.text);
    } else fail("registered task step");
  }

  // Reserve every supplied string, including unknown and forward references.
  // An internal name must never make an otherwise invalid reference meaningful.
  const reserved = new Set<string>();
  function reserve(value: Json): void {
    if (typeof value === "string") reserved.add(value);
    else if (value !== null && typeof value === "object") Object.values(value).forEach(reserve);
  }
  reserve(data);
  let sequence = 0;
  const internal = (): string => {
    let result;
    do { result = `_registered_${sequence++}`; } while (reserved.has(result));
    reserved.add(result);
    return result;
  };
  const expanded: Json[] = [];
  let calls = 0;
  const append = (step: RecordValue): void => {
    if (step.kind === "call") calls++;
    require_(expanded.length < 21 && calls <= 16, "expanded registered task step or call bound");
    expanded.push(step);
  };
  const call = (binding: string, operation: string, arguments_: Record<string, Json>): void => append({
    kind: "call", binding, operation,
    arguments: Object.entries(arguments_).map(([name, value]) => ({ name, value })),
  });
  const bind = (name: string): Json => ({ kind: "binding", name });
  const field = (binding: string, name: string): Json => ({ kind: "field", binding, name });
  const value = (value: string | boolean): Json => ({ kind: "literal", value: { kind: typeof value === "string" ? "string" : "boolean", value } });
  const equal = (left: Json, right: Json): Json => ({ kind: "equal", left, right });
  const guard = (conditions: Json[]): void => append({ kind: "guard", conditions, failure: data.failure! });
  for (const step of steps) {
    if (step.kind === "call" || step.kind === "guard") { append(step); continue; }
    const target = internal();
    call(target, "gui.find", { name: value(step.control as string) });
    if (step.kind === "read_control") {
      call(step.binding as string, "gui.observe", { target: bind(target) });
      continue;
    }
    const before = step.before as string | undefined ?? internal();
    const after = step.binding as string;
    const written = step.kind === "set_control_text";
    call(before, "gui.observe", { target: bind(target) });
    const conditions = [equal(field(before, "role"), value(written ? "text_field" : "button")),
      equal(field(before, "enabled"), value(true))];
    if (written) conditions.push(equal(field(before, "editable"), value(true)));
    guard(conditions);
    const ack = internal();
    const arguments_: Record<string, Json> = { target: bind(target), expected_revision: field(before, "revision") };
    if (written) arguments_.text = step.text!;
    call(ack, written ? "gui.set_text" : "gui.click", arguments_);
    guard([equal(bind(ack), value(true))]);
    call(after, "gui.observe", { target: bind(target) });
    if (written) guard([equal(field(after, "role"), value("text_field")), equal(field(after, "value"), step.text!)]);
  }
  const result = { id: data.id!, summary: data.summary!, inputs, steps: expanded,
    result: data.result!, execution: data.execution!, verification };
  // Expansion consumes the existing total data budget too; do not rely only on
  // the smaller authoring input or on downstream serialization to bound it.
  return freeze(ownDeclaration(result, LIMIT)) as unknown as SemanticTaskDescriptionV2 | SemanticTaskDescriptionV3;
}
