import type { WorkflowPlan } from "./session.js";

/** Bounded data comparison; never invoke accessors, toJSON hooks or coercions. */
function canonical(value: unknown): string {
  const encoder = new TextEncoder();
  const seen = new Set<object>();
  let nodes = 0;
  let bytes = 0;
  function copy(value: unknown, depth: number): unknown {
    if (++nodes > 8192 || depth > 32) throw new Error("invalid_application_evidence");
    if (value === null || typeof value === "boolean") return value;
    if (typeof value === "number" && Number.isSafeInteger(value)) return value;
    if (typeof value === "string") {
      bytes += encoder.encode(value).length;
      if (bytes > 128 * 1024) throw new Error("invalid_application_evidence");
      for (let index = 0; index < value.length; index += 1) {
        const unit = value.charCodeAt(index);
        if (unit >= 0xd800 && unit <= 0xdbff) {
          const next = value.charCodeAt(++index);
          if (!(next >= 0xdc00 && next <= 0xdfff)) throw new Error("invalid_application_evidence");
        } else if (unit >= 0xdc00 && unit <= 0xdfff) throw new Error("invalid_application_evidence");
      }
      return value;
    }
    if (typeof value !== "object" || value === null || seen.has(value)) throw new Error("invalid_application_evidence");
    seen.add(value);
    try {
      if (Array.isArray(value)) {
        if (value.length > 8192 || Reflect.ownKeys(value).length !== value.length + 1) throw new Error("invalid_application_evidence");
        return Array.from({ length: value.length }, (_, index) => {
          const field = Object.getOwnPropertyDescriptor(value, String(index));
          if (!field || !("value" in field) || !field.enumerable) throw new Error("invalid_application_evidence");
          return copy(field.value, depth + 1);
        });
      }
      if (![Object.prototype, null].includes(Object.getPrototypeOf(value))) throw new Error("invalid_application_evidence");
      const keys = Reflect.ownKeys(value);
      if (keys.length > 8192 || keys.some((key) => typeof key !== "string")) throw new Error("invalid_application_evidence");
      const result: Record<string, unknown> = Object.create(null);
      for (const key of (keys as string[]).sort()) {
        const field = Object.getOwnPropertyDescriptor(value, key);
        if (!field || !("value" in field) || !field.enumerable) throw new Error("invalid_application_evidence");
        copy(key, depth + 1);
        result[key] = copy(field.value, depth + 1);
      }
      return result;
    } finally { seen.delete(value); }
  }
  const result = JSON.stringify(copy(value, 0));
  if (encoder.encode(result).length > 128 * 1024) throw new Error("invalid_application_evidence");
  return result;
}

export function snapshots(value: unknown): string {
  // Own once before inspecting identifiers. Observations are registered-control
  // records without dependencies on particular application control names.
  const rows: unknown = JSON.parse(canonical(value));
  if (!Array.isArray(rows) || rows.length > 64) throw new Error("invalid_application_observation");
  const identifiers = new Set<string>();
  const encoder = new TextEncoder();
  const fields = ["identifier", "role", "title", "value", "enabled", "editable", "clicks"];
  for (const row of rows) {
    if (!row || typeof row !== "object" || Array.isArray(row)
      || Object.keys(row).length !== fields.length || Object.keys(row).some((key) => !fields.includes(key))
      || typeof row.enabled !== "boolean" || typeof row.editable !== "boolean"
      || !Number.isInteger(row.clicks) || row.clicks < 0 || row.clicks > 0xffff_ffff) {
      throw new Error("invalid_application_observation");
    }
    for (const field of ["identifier", "role", "title", "value"]) {
      if (typeof row[field] !== "string" || encoder.encode(row[field]).length > 4096) {
        throw new Error("invalid_application_observation");
      }
    }
    if (!row.identifier || identifiers.has(row.identifier)) throw new Error("invalid_application_observation");
    identifiers.add(row.identifier);
  }
  rows.sort((left, right) => left.identifier < right.identifier ? -1 : left.identifier > right.identifier ? 1 : 0);
  return canonical(rows);
}

export function samePlan(left: WorkflowPlan, right: WorkflowPlan): boolean {
  return left.task === right.task && left.source === right.source
    && left.inputMode === right.inputMode
    && canonical(left.arguments) === canonical(right.arguments);
}

/** Call only after lossless semantic verification of the complete raw report. */
export function observations(report: string, fields: { before: string; after: string }): { before: string; after: string } {
  // JSON.parse inspects only registered controls, whose counts are bounded to
  // u32 by snapshots(). Rounded u64 result numbers never participate here.
  const record: unknown = JSON.parse(report);
  if (!record || typeof record !== "object" || Array.isArray(record)
    || !Object.hasOwn(record, fields.before) || !Object.hasOwn(record, fields.after)) {
    throw new Error("invalid_application_evidence");
  }
  const data = record as Record<string, unknown>;
  return { before: snapshots(data[fields.before]), after: snapshots(data[fields.after]) };
}
