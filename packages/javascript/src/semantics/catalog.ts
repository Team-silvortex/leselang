/** Admit complete semantic declarations against their actual typed catalog. */
import { bool, fail, list, record, require_, text, uint, type Json, type RecordValue, type ScalarType } from "./json.js";

export type Type = ScalarType | `native:${string}`;
interface Parameter { name: string; required: boolean; types: Type[] }
interface Operation { name: string; capability: string; parameters: Parameter[]; result: Type }
export interface Catalog {
  profile: string;
  semanticVersion: bigint;
  operations: Map<string, Operation>;
  natives: Map<string, Map<string, ScalarType>>;
  tasks: Map<string, RecordValue>;
}

const scalarTypes: readonly ScalarType[] = ["integer", "boolean", "string", "none", "optional_string", "string_list"];
const reserved = new Set(["fn", "true", "false", "none"]);
function identifier(value: Json | undefined, limit = 64): string {
  const result = text(value, limit, true);
  if (!/^[A-Za-z_][A-Za-z_0-9]*$/u.test(result) || reserved.has(result)) fail("identifier");
  return result;
}
function semanticName(value: Json | undefined): string {
  const result = identifier(value);
  if (result === "body" || result.startsWith("_lese_")) fail("reserved semantic identifier");
  return result;
}
function operationName(value: Json | undefined): string {
  const result = text(value, 128, true);
  for (const part of result.split(".")) identifier(part, 128);
  return result;
}
function scalarType(value: Json | undefined): ScalarType {
  if (!scalarTypes.includes(value as ScalarType)) fail("scalar type");
  return value as ScalarType;
}
function literalType(value: Json | undefined): ScalarType {
  const data = record(value);
  const kind = scalarType(data.kind);
  record(data, kind === "none" ? ["kind"] : ["kind", "value"]);
  if (kind === "string") text(data.value, 4096);
  else if (kind === "integer") uint(data.value);
  else if (kind === "boolean") bool(data.value);
  else if (kind !== "none") fail("unsupported semantic literal");
  return kind;
}

function validateReportPath(value: Json | undefined): Json[] {
  const segments = list(value, 8, true);
  for (const raw of segments) {
    const segment = record(raw);
    if (segment.kind === "field") {
      record(segment, ["kind", "name"]); text(segment.name, 128, true);
    } else if (segment.kind === "match") {
      record(segment, ["kind", "field", "value"]); text(segment.field, 128, true); text(segment.value, 4096);
    } else fail("report path segment");
  }
  return segments;
}

/** Fixed v1 results or explicitly typed independent observations in v2. */
export function verificationResultType(value: Json | undefined, version: bigint): ScalarType {
  const result = record(value);
  if (result.kind !== "observed") return literalType(result);
  require_(version === 2n, "observed results require semantic version 2");
  record(result, ["kind", "scalar_type", "path"]);
  require_(result.scalar_type === "string" || result.scalar_type === "integer" || result.scalar_type === "boolean", "observed result scalar type");
  const segments = validateReportPath(result.path);
  const first = record(segments[0]);
  require_(first.kind === "field" && first.name !== "result", "observed result must select independent report evidence");
  return result.scalar_type as ScalarType;
}
function nativeType(value: Json | undefined, natives: Catalog["natives"]): Type {
  const data = record(value);
  if (data.kind === "scalar") return scalarType(data.name);
  if (data.kind !== "native") fail("catalog type kind");
  const name = text(data.name, 64, true);
  if (!natives.has(name)) fail("undeclared native type");
  return `native:${name}`;
}
function unique<T>(values: T[], key: (value: T) => string): Map<string, T> {
  const result = new Map<string, T>();
  for (const value of values) {
    const name = key(value);
    if (result.has(name)) fail("duplicate name");
    result.set(name, value);
  }
  return result;
}

function termType(value: Json | undefined, catalog: Catalog, bindings: Map<string, Type>, input?: string): Type {
  const term = record(value);
  switch (term.kind) {
    case "literal": record(term, ["kind", "value"]); return literalType(term.value);
    case "input": {
      record(term, ["kind", "name"]);
      if (!input || term.name !== input) fail("undeclared task input");
      return "string";
    }
    case "binding": {
      record(term, ["kind", "name"]);
      const type = bindings.get(text(term.name, 64, true));
      if (!type) fail("unknown or forward binding");
      return type;
    }
    case "field": {
      record(term, ["kind", "binding", "name"]);
      const binding = bindings.get(text(term.binding, 64, true));
      if (!binding?.startsWith("native:")) fail("field requires native binding");
      const type = catalog.natives.get(binding.slice(7))?.get(text(term.name, 128, true));
      if (!type) fail("undeclared scalar field");
      return type;
    }
    default: return fail("term kind");
  }
}

function validateCondition(value: Json, catalog: Catalog, bindings: Map<string, Type>, input?: string): void {
  const condition = record(value);
  const type = (term: Json | undefined): ScalarType => {
    const result = termType(term, catalog, bindings, input);
    if (result.startsWith("native:")) fail("native scalar comparison");
    return result as ScalarType;
  };
  switch (condition.kind) {
    case "equal":
      record(condition, ["kind", "left", "right"]);
      require_(type(condition.left) === type(condition.right), "condition type mismatch"); break;
    case "contains":
      record(condition, ["kind", "list", "value"]);
      require_(type(condition.list) === "string_list" && type(condition.value) === "string", "contains types"); break;
    case "increased_by":
      record(condition, ["kind", "before", "after", "amount"]); uint(condition.amount);
      require_(type(condition.before) === "integer" && type(condition.after) === "integer", "increase types"); break;
    case "optional_equals":
      record(condition, ["kind", "optional", "value"]);
      require_(type(condition.optional) === "optional_string" && type(condition.value) === "string", "optional comparison types"); break;
    default: fail("condition kind");
  }
}

function reportTermType(value: Json | undefined, input?: string): ScalarType | undefined {
  const term = record(value);
  switch (term.kind) {
    case "literal": record(term, ["kind", "value"]); return literalType(term.value);
    case "input":
      record(term, ["kind", "name"]); require_(input !== undefined && term.name === input, "report input"); return "string";
    case "path": case "length": case "scalar": {
      record(term, term.kind === "scalar" ? ["kind", "path", "scalar_type"] : ["kind", "path"]);
      validateReportPath(term.path);
      return term.kind === "length" ? "integer" : term.kind === "scalar" ? scalarType(term.scalar_type) : undefined;
    }
    default: return fail("report term kind");
  }
}

export function taskInput(task: RecordValue): string | undefined {
  const inputs = list(task.inputs, 1);
  if (inputs.length === 0) return undefined;
  const input = record(inputs[0], ["name", "scalar_type"]);
  require_(input.scalar_type === "string", "v1 input must be string");
  return semanticName(input.name);
}

function validateTask(task: RecordValue, catalog: Catalog, operations: Set<string>): void {
  record(task, ["id", "summary", "inputs", "steps", "result", "execution", "verification"]);
  semanticName(task.id); text(task.summary, 1024, true);
  const input = taskInput(task);
  require_(["named_text", "text", "no_text"].includes(text(task.execution, 16)), "execution style");
  require_((task.execution === "no_text") === (input === undefined), "input convention mismatch");
  const verification = record(task.verification, ["kind", "max_fuel", "result", "require_verified", "predicates"]);
  text(verification.kind, 128, true); require_(uint(verification.max_fuel, 1_000_000n) > 0n, "Fuel ceiling");
  bool(verification.require_verified);
  const resultType = verificationResultType(verification.result, catalog.semanticVersion);
  const bindings = new Map<string, Type>();
  let calls = 0;
  for (const raw of list(task.steps, 21, true)) {
    const step = record(raw);
    if (step.kind === "call") {
      record(step, ["kind", "binding", "operation", "arguments"]);
      const binding = semanticName(step.binding);
      require_(binding !== input && !bindings.has(binding), "duplicate binding");
      const operation = catalog.operations.get(text(step.operation, 128, true));
      if (!operation || !operations.has(operation.name) || ++calls > 16) fail("task operation reference or bound");
      const arguments_ = unique(list(step.arguments, 64).map((argument) => record(argument, ["name", "value"])), (argument) => text(argument.name, 64, true));
      for (const [name, argument] of arguments_) {
        const parameter = operation.parameters.find((parameter) => parameter.name === name);
        if (!parameter || !parameter.types.includes(termType(argument.value, catalog, bindings, input))) fail("task argument type");
      }
      for (const parameter of operation.parameters) require_(!parameter.required || arguments_.has(parameter.name), "missing task argument");
      bindings.set(binding, operation.result);
    } else if (step.kind === "guard") {
      record(step, ["kind", "conditions", "failure"]);
      require_(literalType(step.failure) === resultType, "guard result type");
      for (const condition of list(step.conditions, 8, true)) validateCondition(condition, catalog, bindings, input);
    } else fail("step kind");
  }
  require_(termType(task.result, catalog, bindings, input) === resultType, "task result type");
  const predicates = unique(list(verification.predicates, 64, true).map((value) => record(value, ["id", "left", "relation", "right"])), (value) => semanticName(value.id));
  for (const predicate of predicates.values()) {
    const left = reportTermType(predicate.left, input), right = reportTermType(predicate.right, input);
    const relation = record(predicate.relation);
    if (relation.kind === "equal") {
      record(relation, ["kind"]);
      require_(left === undefined || right === undefined || left === right, "predicate type mismatch");
    } else if (relation.kind === "at_most" || relation.kind === "increased_by") {
      record(relation, relation.kind === "at_most" ? ["kind"] : ["kind", "amount"]);
      if (relation.kind === "increased_by") uint(relation.amount);
      require_((left === undefined || left === "integer") && (right === undefined || right === "integer"), "integer predicate type");
    } else fail("predicate relation");
  }
}

export function validate(raw: Json): Catalog {
  const data = record(raw);
  require_(uint(data.schema_version) === 1n && uint(data.protocol_version) === 1n, "catalog version");
  require_(uint(data.catalog_version, 0xffff_ffffn) > 0n, "catalog version");
  const profile = text(data.profile, 128, true);
  require_(/^[A-Za-z0-9_.-]+$/u.test(profile), "profile name");
  const natives = new Map<string, Map<string, ScalarType>>();
  for (const raw of list(data.native_types, 32)) {
    const native = record(raw), name = identifier(native.name);
    require_(name !== "body" && !scalarTypes.includes(name as ScalarType) && !natives.has(name), "native alias");
    const fields = new Map<string, ScalarType>();
    for (const raw of list(native.fields, 64)) {
      const field = record(raw), name = text(field.name, 128, true);
      require_(!fields.has(name), "duplicate field"); fields.set(name, scalarType(field.scalar_type));
    }
    natives.set(name, fields);
  }
  const operations = unique(list(data.operations, 64).map((raw): Operation => {
    const operation = record(raw);
    const parameters = unique(list(operation.parameters, 64).map((raw): Parameter => {
      const parameter = record(raw);
      const types = list(parameter.types, 38).map((type) => nativeType(type, natives));
      require_(new Set(types).size === types.length, "duplicate parameter type");
      return { name: identifier(parameter.name), required: bool(parameter.required), types };
    }), (parameter) => parameter.name);
    return { name: operationName(operation.name), capability: text(operation.required_capability, 128, true),
      parameters: [...parameters.values()], result: nativeType(operation.result, natives) };
  }), (operation) => operation.name);
  const semantics = record(data.semantics, ["version", "operations", "tasks"]);
  const semanticVersion = uint(semantics.version);
  require_(semanticVersion === 1n || semanticVersion === 2n, "semantic version");
  const claims = unique(list(semantics.operations, 64).map((raw) => {
    const operation = record(raw, ["operation", "summary", "effect", "completion"]);
    const name = text(operation.operation, 128, true);
    require_(operations.has(name), "semantic operation reference"); text(operation.summary, 1024, true);
    require_(operation.effect === "read" || operation.effect === "write", "effect classification");
    require_(operation.completion === "value" || operation.completion === "submission", "completion classification");
    return operation;
  }), (operation) => operation.operation as string);
  const tasks = unique(list(semantics.tasks, 8, true).map((raw) => record(raw)), (task) => semanticName(task.id));
  const catalog: Catalog = { profile, semanticVersion, operations, natives, tasks };
  for (const task of tasks.values()) validateTask(task, catalog, new Set(claims.keys()));
  return catalog;
}

