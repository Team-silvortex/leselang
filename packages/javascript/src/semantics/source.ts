/** Generate bounded LeseLang source from already-admitted declarations. */
import { taskInput, type Catalog, type Type } from "./catalog.js";
import { fail, list, record, require_, text, type Json, type RecordValue } from "./json.js";

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

export function generate(catalog: Catalog, task: RecordValue): string {
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

