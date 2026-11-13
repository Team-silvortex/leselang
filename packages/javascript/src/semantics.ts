/** Host-authored, bounded semantic tasks. This module never executes an Effect. */
import type { WorkflowPlan } from "./agent.js";

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

type Json = null | boolean | string | number | bigint | Json[] | RecordValue;
interface RecordValue { [name: string]: Json }
type ScalarType = "integer" | "boolean" | "string" | "none" | "optional_string" | "string_list";
type Type = ScalarType | `native:${string}`;
interface Parameter { name: string; required: boolean; types: Type[] }
interface Operation { name: string; capability: string; parameters: Parameter[]; result: Type }
interface Catalog {
  profile: string;
  operations: Map<string, Operation>;
  natives: Map<string, Map<string, ScalarType>>;
  tasks: Map<string, RecordValue>;
}

const encoder = new TextEncoder();
const U64_MAX = 18_446_744_073_709_551_615n;
const scalarTypes: readonly ScalarType[] = ["integer", "boolean", "string", "none", "optional_string", "string_list"];
const reserved = new Set(["fn", "true", "false", "none"]);
function fail(reason: string): never { throw new Error(`invalid_semantics: ${reason}`); }
function require_(valid: boolean, reason: string): void { if (!valid) fail(reason); }

function text(value: unknown, limit: number, nonempty = false): string {
  if (typeof value !== "string" || value.length > limit || (nonempty && value.length === 0)) fail("text bounds or type");
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) fail("invalid UTF-16");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) fail("invalid UTF-16");
  }
  if (encoder.encode(value).length > limit) fail("UTF-8 text bound");
  return value;
}

/** Preserve integer tokens exactly; never recover a u64 from a rounded Number. */
function decode(value: unknown, limit: number): Json {
  let nodes = 0;
  const node = (depth: number) => {
    if (++nodes > 65_536 || depth > 32) fail("JSON structure bound");
  };
  if (typeof value === "string") {
    const source = text(value, limit, true);
    let index = 0;
    const whitespace = () => { while (/[ \t\r\n]/u.test(source[index] ?? "!")) index += 1; };
    const string = (): string => {
      const start = index++;
      while (index < source.length) {
        const char = source[index++]!;
        if (char === "\\") { index += 1; continue; }
        if (char === '"') {
          try { return text(JSON.parse(source.slice(start, index)), limit); }
          catch { return fail("JSON string"); }
        }
      }
      return fail("unterminated JSON string");
    };
    const parse = (depth: number): Json => {
      node(depth); whitespace();
      const char = source[index];
      if (char === '"') return string();
      if (char === "[") {
        index += 1; whitespace();
        const values: Json[] = [];
        if (source[index] === "]") { index += 1; return values; }
        while (true) {
          values.push(parse(depth + 1)); whitespace();
          if (source[index] === "]") { index += 1; return values; }
          if (source[index++] !== ",") fail("JSON array separator");
        }
      }
      if (char === "{") {
        index += 1; whitespace();
        const fields: RecordValue = Object.create(null);
        if (source[index] === "}") { index += 1; return fields; }
        while (true) {
          whitespace();
          if (source[index] !== '"') fail("JSON object key");
          const key = string();
          if (Object.hasOwn(fields, key)) fail("duplicate JSON key");
          whitespace();
          if (source[index++] !== ":") fail("JSON object separator");
          fields[key] = parse(depth + 1); whitespace();
          if (source[index] === "}") { index += 1; return fields; }
          if (source[index++] !== ",") fail("JSON object separator");
        }
      }
      for (const [token, result] of [["true", true], ["false", false], ["null", null]] as const) {
        if (source.startsWith(token, index)) { index += token.length; return result; }
      }
      const match = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/u.exec(source.slice(index));
      if (!match) fail("JSON value");
      index += match[0].length;
      // Float/exponent lexemes remain distinct from integer tokens, as in serde.
      if (/[.eE]/u.test(match[0]) || match[0] === "-0") {
        const number = Number(match[0]);
        if (!Number.isFinite(number)) fail("JSON number range");
        return number;
      }
      return BigInt(match[0]);
    };
    const result = parse(0); whitespace();
    if (index !== source.length) fail("trailing JSON data");
    return result;
  }

  const seen = new Set<object>();
  let bytes = 0;
  const spend = (count: number) => { if ((bytes += count) > limit) fail("JSON byte bound"); };
  const stringBytes = (value: string) => encoder.encode(JSON.stringify(value)).length;
  const copy = (value: unknown, depth: number): Json => {
    node(depth);
    if (value === null || typeof value === "boolean") { spend(value === null || value === true ? 4 : 5); return value; }
    if (typeof value === "string") { const checked = text(value, limit); spend(stringBytes(checked)); return checked; }
    if (typeof value === "number") {
      if (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value))) fail("unsafe JSON number");
      spend(String(value).length);
      return Number.isInteger(value) ? BigInt(value) : value;
    }
    if (typeof value !== "object" || seen.has(value)) fail("JSON value or cycle");
    seen.add(value);
    try {
      const keys = Reflect.ownKeys(value);
      if (Array.isArray(value)) {
        if (value.length > 65_536 || keys.length !== value.length + 1) fail("JSON array properties");
        spend(2 + Math.max(0, value.length - 1));
        return Array.from({ length: value.length }, (_, index) => {
          const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
          if (!descriptor?.enumerable || !("value" in descriptor)) fail("JSON array accessor or hole");
          return copy(descriptor.value, depth + 1);
        });
      }
      if (![Object.prototype, null].includes(Object.getPrototypeOf(value))) fail("JSON record prototype");
      spend(2 + Math.max(0, keys.length - 1) + keys.length);
      const fields: RecordValue = Object.create(null);
      for (const key of keys) {
        if (typeof key !== "string") fail("JSON symbol key");
        const descriptor = Object.getOwnPropertyDescriptor(value, key);
        if (!descriptor?.enumerable || !("value" in descriptor)) fail("JSON record accessor");
        const checked = text(key, limit); spend(stringBytes(checked));
        fields[checked] = copy(descriptor.value, depth + 1);
      }
      return fields;
    } finally { seen.delete(value); }
  };
  return copy(value, 0);
}

function record(value: Json | undefined, exact?: readonly string[]): RecordValue {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail("record expected");
  if (exact) {
    const keys = Object.keys(value);
    if (keys.length !== exact.length || keys.some((key) => !exact.includes(key))) fail("unknown or missing semantic field");
  }
  return value;
}
function list(value: Json | undefined, max: number, nonempty = false): Json[] {
  if (!Array.isArray(value) || value.length > max || (nonempty && value.length === 0)) fail("collection bound or type");
  return value;
}
function uint(value: Json | undefined, max = U64_MAX): bigint {
  if (typeof value !== "bigint" || value < 0n || value > max) fail("unsigned integer type or bound");
  return value;
}
function bool(value: Json | undefined): boolean {
  if (typeof value !== "boolean") fail("Boolean expected");
  return value;
}
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
      for (const raw of list(term.path, 8, true)) {
        const segment = record(raw);
        if (segment.kind === "field") {
          record(segment, ["kind", "name"]); text(segment.name, 128, true);
        } else if (segment.kind === "match") {
          record(segment, ["kind", "field", "value"]); text(segment.field, 128, true); text(segment.value, 4096);
        } else fail("report path segment");
      }
      return term.kind === "length" ? "integer" : term.kind === "scalar" ? scalarType(term.scalar_type) : undefined;
    }
    default: return fail("report term kind");
  }
}

function taskInput(task: RecordValue): string | undefined {
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
  const resultType = literalType(verification.result);
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

function validate(raw: Json): Catalog {
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
  require_(uint(semantics.version) === 1n, "semantic version");
  const claims = unique(list(semantics.operations, 64).map((raw) => {
    const operation = record(raw, ["operation", "summary", "effect", "completion"]);
    const name = text(operation.operation, 128, true);
    require_(operations.has(name), "semantic operation reference"); text(operation.summary, 1024, true);
    require_(operation.effect === "read" || operation.effect === "write", "effect classification");
    require_(operation.completion === "value" || operation.completion === "submission", "completion classification");
    return operation;
  }), (operation) => operation.operation as string);
  const tasks = unique(list(semantics.tasks, 8, true).map((raw) => record(raw)), (task) => semanticName(task.id));
  const catalog: Catalog = { profile, operations, natives, tasks };
  for (const task of tasks.values()) validateTask(task, catalog, new Set(claims.keys()));
  return catalog;
}

function quote(value: string): string {
  return `"${value.replaceAll("\\", "\\\\").replaceAll('"', '\\"').replaceAll("\n", "\\n").replaceAll("\r", "\\r").replaceAll("\t", "\\t")}"`;
}
function literal(value: Json | undefined): string {
  const data = record(value);
  if (data.kind === "none") return "none";
  return data.kind === "string" ? quote(data.value as string) : String(data.value);
}
function term(value: Json | undefined): string {
  const data = record(value);
  if (data.kind === "literal") return literal(data.value);
  if (data.kind === "field") return `field(value: ${data.binding}, name: ${quote(data.name as string)})`;
  return data.name as string;
}
function condition(value: Json): string {
  const data = record(value);
  switch (data.kind) {
    case "equal": return `eq(left: ${term(data.left)}, right: ${term(data.right)})`;
    case "contains": return `_lese_contains(items: ${term(data.list)}, wanted: ${term(data.value)})`;
    case "increased_by": return `eq(left: ${term(data.after)}, right: add(left: ${term(data.before)}, right: ${data.amount}))`;
    case "optional_equals": {
      const optional = term(data.optional);
      return `and(left: has_value(value: ${optional}), right: eq(left: value_or(left: ${optional}, right: ""), right: ${term(data.value)}))`;
    }
    default: return fail("unvalidated condition");
  }
}
function all(values: string[]): string {
  if (values.length === 1) return values[0]!;
  const middle = Math.floor(values.length / 2);
  return `and(left: ${all(values.slice(0, middle))}, right: ${all(values.slice(middle))})`;
}
function uses(value: Json | undefined, names: Set<string>): void {
  if (!value || typeof value !== "object") return;
  if (Array.isArray(value)) { value.forEach((value) => uses(value, names)); return; }
  if (value.kind === "input" || value.kind === "binding") names.add(value.name as string);
  else if (value.kind === "field" && typeof value.binding === "string") names.add(value.binding);
  else Object.values(value).forEach((value) => uses(value, names));
}
function sourceDepth(source: string): void {
  let depth = 0, quoted = false, escaped = false, comment = false;
  for (let index = 0; index < source.length; index += 1) {
    const character = source[index]!;
    if (comment) { if (character === "\n") comment = false; continue; }
    if (quoted) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === '"') quoted = false;
      continue;
    }
    if (character === "/" && source[index + 1] === "/") { comment = true; index += 1; }
    else if (character === '"') quoted = true;
    else if (character === "(") { if (++depth > 16) fail("generated call depth"); }
    else if (character === ")") { if (--depth < 0) fail("generated call depth"); }
  }
  if (depth !== 0 || quoted) fail("generated source structure");
}

function generate(catalog: Catalog, task: RecordValue): string {
  const steps = list(task.steps, 21).map((step) => record(step));
  require_(!steps.some((step) => step.kind === "call" && (step.operation === "main" || (step.operation as string).startsWith("_lese_"))), "generated operation collision");
  const known = new Map<string, Type>();
  const input = taskInput(task);
  if (input) known.set(input, "string");
  const chunks: RecordValue[][] = [];
  const parameters: [string, Type][][] = [];
  for (let start = 0; start < steps.length; start += 9) {
    const live = new Set<string>(); uses(steps.slice(start), live); uses(task.result, live);
    const selected = [...known].filter(([name]) => start === 0 || live.has(name)).sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0);
    if (selected.length > 8) fail("helper parameter bound"); parameters.push(selected);
    const chunk = steps.slice(start, start + 9); chunks.push(chunk);
    for (const step of chunk) if (step.kind === "call") known.set(step.binding as string, catalog.operations.get(step.operation as string)!.result);
  }
  const contains = steps.some((step) => step.kind === "guard" && list(step.conditions, 8).some((condition) => record(condition).kind === "contains"));
  if (chunks.length + Number(contains) > 8) fail("helper count bound");
  let source = "// Generated from validated host semantic metadata; no retry or scheduler.\n";
  if (contains) source += 'fn _lese_contains(items: string_list, wanted: string) = fold(\n  _lese_found: false, items: items, item: "_lese_item",\n  next: or(left: _lese_found, right: eq(left: _lese_item, right: wanted)), limit: 64\n)\n';
  chunks.forEach((chunk, index) => {
    let body = index + 1 === chunks.length ? term(task.result)
      : `_lese_stage_${index + 1}(${parameters[index + 1]!.map(([name]) => `${name}: ${name}`).join(", ")})`;
    for (const step of [...chunk].reverse()) {
      if (step.kind === "call") {
        const supplied = list(step.arguments, 64).map((argument) => record(argument));
        const arguments_ = catalog.operations.get(step.operation as string)!.parameters.flatMap((parameter) => {
          const argument = supplied.find((argument) => argument.name === parameter.name);
          return argument ? [`${parameter.name}: ${term(argument.value)}`] : [];
        }).join(", ");
        body = `bind(${step.binding}: ${step.operation}(${arguments_}), body: ${body})`;
      } else body = `choose(when: ${all(list(step.conditions, 8).map(condition))}, then: ${body}, otherwise: ${literal(step.failure)})`;
    }
    const name = index === 0 ? "main" : `_lese_stage_${index}`;
    const arguments_ = parameters[index]!.map(([name, type]) => `${name}: ${type.startsWith("native:") ? type.slice(7) : type}`).join(", ");
    source += `fn ${name}(${arguments_}) = ${body}\n`;
  });
  text(source, 16 * 1024, true); sourceDepth(source);
  return source;
}

function scalar(value: Json | undefined, type: ScalarType): Json {
  if (value === undefined) fail("missing report scalar");
  switch (type) {
    case "integer": uint(value); break;
    case "boolean": bool(value); break;
    case "string": text(value, 4096); break;
    case "none": require_(value === null, "report none"); break;
    case "optional_string": if (value !== null) text(value, 4096); break;
    case "string_list": {
      const values = list(value, 64); let length = 0;
      for (const value of values) length += encoder.encode(text(value, 4096)).length;
      require_(length <= 4096, "string list bound"); break;
    }
  }
  return value;
}
function literalJson(value: Json | undefined): Json {
  const data = record(value); return data.kind === "none" ? null : data.value!;
}
function equal(left: Json, right: Json): boolean {
  if (typeof left !== typeof right) return false;
  if (left === null || right === null || typeof left !== "object" || typeof right !== "object") return left === right;
  if (Array.isArray(left) || Array.isArray(right)) return Array.isArray(left) && Array.isArray(right)
    && left.length === right.length && left.every((value, index) => equal(value, right[index]!));
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every((key) => Object.hasOwn(right, key) && equal(left[key]!, right[key]!));
}
function path(report: Json, segments: Json[]): Json {
  let value = report;
  for (const raw of segments) {
    const segment = record(raw);
    if (segment.kind === "field") {
      const fields = record(value);
      if (!Object.hasOwn(fields, segment.name as string)) fail("missing report field");
      value = fields[segment.name as string]!;
    } else {
      const matches = list(value, 65_536).filter((item) => item !== null && typeof item === "object" && !Array.isArray(item)
        && item[segment.field as string] === segment.value);
      if (matches.length !== 1) fail("missing or ambiguous report selection");
      value = matches[0]!;
    }
  }
  return value;
}
function reportTerm(report: Json, value: Json | undefined, args: Readonly<Record<string, string>>): Json {
  const term = record(value);
  if (term.kind === "literal") return literalJson(term.value);
  if (term.kind === "input") return args[term.name as string]!;
  const selected = path(report, list(term.path, 8));
  if (term.kind === "length") return BigInt(list(selected, 65_536).length);
  if (term.kind === "scalar") return scalar(selected, term.scalar_type as ScalarType);
  return selected;
}
function reportBounds(value: Json): void {
  if (typeof value === "string") text(value, 4096);
  else if (Array.isArray(value)) value.forEach(reportBounds);
  else if (value !== null && typeof value === "object") {
    for (const [key, entry] of Object.entries(value)) { text(key, 4096); reportBounds(entry); }
  }
}
function verify(catalog: Catalog, task: RecordValue, args: Readonly<Record<string, string>>, effects: number, raw: unknown): boolean {
  try {
    const report = record(decode(raw, 128 * 1024)); reportBounds(report);
    const contract = record(task.verification);
    require_(uint(report.schema_version) === 1n && report.profile === catalog.profile && report.kind === contract.kind, "report identity");
    require_(uint(report.effects) === BigInt(effects) && uint(report.fuel_remaining) <= uint(contract.max_fuel), "report budget");
    if (contract.require_verified) require_(report.verified === true, "host verification");
    const result = record(report.result), expected = record(contract.result);
    require_(result.type === expected.kind && Object.hasOwn(result, "value") && equal(result.value!, literalJson(expected)), "typed report result");
    for (const raw of list(contract.predicates, 64)) {
      const predicate = record(raw), relation = record(predicate.relation);
      const left = reportTerm(report, predicate.left, args), right = reportTerm(report, predicate.right, args);
      let satisfied: boolean;
      if (relation.kind === "equal") satisfied = equal(left, right);
      else if (relation.kind === "at_most") satisfied = uint(left) <= uint(right);
      else { const sum = uint(left) + uint(relation.amount); satisfied = sum <= U64_MAX && sum === uint(right); }
      require_(satisfied, `report predicate ${predicate.id}`);
    }
    return true;
  } catch { return false; }
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
