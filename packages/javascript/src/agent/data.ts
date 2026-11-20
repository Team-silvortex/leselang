import type { AgentJson, WorkflowPlan } from "./session.js";

const encoder = new TextEncoder();
const JSON_LIMIT = 128 * 1024;
const JSON_NODE_LIMIT = 8192;

export function fault(message: string): never { throw new Error(message); }

export function boundedText(value: unknown, limit: number, label: string, nonempty = false): string {
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

export function fields(value: unknown, expected?: readonly string[]): Record<string, unknown> {
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

export function proposal(value: unknown): WorkflowPlan {
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
export function json(value: unknown, label: string, limit = JSON_LIMIT): AgentJson {
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

export function errorText(error: unknown): string {
  try {
    const message = error instanceof Error ? error.message : typeof error === "string" ? error : "agent_task_failed";
    // Only display a bounded valid string; never stringify arbitrary thrown data.
    return boundedText(message, 1024, "error") || "agent_task_failed";
  } catch { return "agent_task_failed"; }
}

