import { Bridge, prepareSemanticCatalog, type AgentHost, type WorkflowPlan } from "./bridge.js";
import type { TauriApi } from "./tauri-api.js";

interface ApplicationConnection {
  attach(bridge: Bridge): void;
  detach(bridge: Bridge): void;
  observe(): unknown;
}
function aborted(signal: AbortSignal): void {
  if (signal.aborted) throw new Error("任务已取消。");
}

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

function snapshots(value: unknown): string {
  // Own once before inspecting identifiers. Application observations contain
  // registered control records, with no dependency on particular control names.
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

function samePlan(left: WorkflowPlan, right: WorkflowPlan): boolean {
  return left.task === right.task && left.source === right.source
    && canonical(left.arguments) === canonical(right.arguments);
}

/** Only inspect bounded public observations after lossless semantic verification. */
function observations(report: string): { before: string; after: string } {
  // The semantic decoder has already admitted the complete raw JSON, including
  // duplicate keys and exact u64 result values. JSON.parse is used solely for
  // registered-control observations, whose clicks are independently bounded to
  // u32 by snapshots(). Rounded result numbers never participate in a check.
  const record: unknown = JSON.parse(report);
  if (!record || typeof record !== "object" || Array.isArray(record)) throw new Error("invalid_application_evidence");
  const fields = record as Record<string, unknown>;
  return { before: snapshots(fields.native_before), after: snapshots(fields.native_after) };
}

/** The application owns the catalog, endpoint, registrations and verification. */
export function createAgentHost(api: TauriApi, application: ApplicationConnection, options: {
  epoch: string;
  onSource?: (source: string) => void;
}): AgentHost & { close(): void } {
  const { epoch } = options;
  let active: { stop: () => void } | null = null;
  let closed = false;
  let catalog: ReturnType<typeof prepareSemanticCatalog> | null = null;
  let completed: { prepared: ReturnType<ReturnType<typeof prepareSemanticCatalog>["prepare"]>; before: string; report: string; live(): boolean } | null = null;
  return {
    async describe(signal) {
      aborted(signal);
      if (closed) throw new Error("应用会话已关闭。");
      catalog = null;
      completed = null;
      // Preserve u64 semantic literals exactly instead of passing rounded JSON numbers.
      const described = await api.core.invoke("agent_describe", { raw: true, taskCatalog: true });
      aborted(signal);
      if (closed) throw new Error("应用会话已关闭。");
      catalog = prepareSemanticCatalog(described);
      return described;
    },
    async execute(plan: WorkflowPlan, signal: AbortSignal) {
      aborted(signal);
      if (closed || active || !catalog || !plan.task) throw new Error("应用会话不可用于新的任务。");
      completed = null;
      const prepared = catalog.prepare(plan.task, plan.arguments);
      if (!samePlan(prepared.plan, plan)) throw new Error("工作流与宿主声明不一致。");
      const before = snapshots(application.observe());
      const session = crypto.randomUUID();
      const bridge = new Bridge({ window: "main", epoch, session });
      const channel = new api.core.Channel<unknown>();
      let stopped = false;
      const stop = () => {
        stopped = true;
        bridge.close();
        try { application.detach(bridge); } catch { /* The closed bridge still fences mutations. */ }
        void api.core.invoke("agent_cancel", { epoch, session }).catch(() => {});
      };
      const current = { stop };
      try {
        application.attach(bridge);
        active = current;
        signal.addEventListener("abort", stop, { once: true });
        channel.onmessage = (request) => {
          void bridge.handle(request)
            .then((reply) => api.core.invoke("agent_reply", { reply }))
            .catch(stop);
        };
        options.onSource?.(prepared.plan.source);
        aborted(signal);
        const report = await api.core.invoke("agent_start_task", {
          onRequest: channel, epoch, session, source: prepared.plan.source, arguments: prepared.plan.arguments,
        });
        if (!signal.aborted && !closed && !stopped && typeof report === "string" && prepared.verify(report)) {
          completed = { prepared, before, report, live: () => !stopped && !signal.aborted };
        }
        return report;
      } finally {
        signal.removeEventListener("abort", stop);
        try {
          bridge.close();
          application.detach(bridge);
        } catch (error) {
          completed = null;
          throw error;
        } finally {
          if (active === current) active = null;
        }
      }
    },
    verify(report, plan, signal) {
      aborted(signal);
      const evidence = completed;
      completed = null;
      if (closed || !evidence || !evidence.live()) return false;
      try {
        if (!samePlan(plan, evidence.prepared.plan) || typeof report !== "string" || report !== evidence.report
          || !evidence.prepared.verify(report)) return false;
        const observed = observations(report);
        return observed.before === evidence.before
          && observed.after === snapshots(application.observe());
      } catch { return false; }
    },
    close() {
      if (closed) return;
      closed = true;
      catalog = null;
      completed = null;
      active?.stop();
      void api.core.invoke("agent_closed", { epoch }).catch(() => {});
    },
  };
}
