import { randomUUID } from "node:crypto";
import { json, proposal } from "../agent/data.js";
import { samePlan } from "../agent/registered-evidence.js";
import type { AgentHost, AgentJson, WorkflowPlan } from "../agent/session.js";
import { readOperationValue, type OperationRegistry, type OperationScalarType,
  type OperationWireValue } from "../operations/index.js";
import { createWorkerTransport, decodeWorkerJson, exact, type NodeAgentTransportOptions } from "./transport.js";

export type OperationProfile = "javascript.operations" | "lese.operations";

export interface OperationTaskReport<P extends OperationProfile = "javascript.operations"> {
  readonly schema_version: 1;
  readonly kind: "operation_task_run";
  readonly profile: P;
  readonly result: { readonly type: OperationScalarType; readonly value: OperationWireValue };
  readonly effects: number;
  readonly fuel_remaining: number;
}

interface HostOptions<P extends OperationProfile> extends Pick<NodeAgentTransportOptions, "executable" | "args" | "timeoutMs" | "onStderr"> {
  readonly registry: OperationRegistry;
  /** Trusted wire and report identity. Omission selects the original v4 profile. */
  readonly profile?: P;
  /** Current application policy. Called at admission and at live handler boundaries. */
  readonly grants: () => readonly string[];
  /** Optional synchronous application snapshot captured before execution and again at verification. */
  readonly observe?: () => AgentJson;
  /** Check application-owned evidence for this exact plan and completed report. */
  readonly verify: (evidence: {
    readonly plan: WorkflowPlan;
    readonly report: OperationTaskReport<P>;
    readonly before: AgentJson | undefined;
    readonly after: AgentJson | undefined;
    readonly signal: AbortSignal;
  }) => boolean | Promise<boolean>;
}

export type NodeOperationHostOptions = HostOptions<"javascript.operations"> |
  (HostOptions<"lese.operations"> & { readonly profile: "lese.operations" });

export interface NodeOperationHost extends AgentHost {
  close(): void;
}

function admitReport<P extends OperationProfile>(raw: string, delivered: number, profile: P): OperationTaskReport<P> {
  const report = decodeWorkerJson(new TextEncoder().encode(raw), 4);
  exact(report, ["schema_version", "kind", "profile", "result", "effects", "fuel_remaining"]);
  exact(report.result, ["type", "value"]);
  if (report.schema_version !== 1 || report.kind !== "operation_task_run" || report.profile !== profile
    || report.effects !== delivered || typeof report.fuel_remaining !== "number" || report.fuel_remaining < 0
    || report.fuel_remaining > 10_000) throw new Error("operation_invalid_report");
  // The decoder preserves decimal strings and admits only safe integer JSON numbers.
  readOperationValue(report.result.type as OperationScalarType, report.result.value);
  if (Array.isArray(report.result.value)) Object.freeze(report.result.value);
  Object.freeze(report.result);
  return Object.freeze(report) as unknown as OperationTaskReport<P>;
}

interface Attempt {
  readonly effects: AbortController;
  readonly grants: ReadonlySet<string>;
}

interface Completion<P extends OperationProfile> {
  readonly raw: string;
  readonly report: OperationTaskReport<P>;
  readonly plan: WorkflowPlan;
  readonly signal: AbortSignal;
  readonly before: AgentJson | undefined;
}

/** Run application-declared scalar operations through the standalone Rust VM. */
export function createNodeOperationHost(options: NodeOperationHostOptions): NodeOperationHost {
  if (options.profile === "lese.operations") return operationHost("lese.operations", options);
  if (options.profile === undefined || options.profile === "javascript.operations") {
    return operationHost("javascript.operations", options);
  }
  throw new Error("operation_invalid_profile");
}

function operationHost<P extends OperationProfile>(profile: P, options: HostOptions<P>): NodeOperationHost {
  const { registry, grants, verify, observe } = options;
  if (typeof grants !== "function" || typeof verify !== "function" || typeof registry?.dispatch !== "function"
    || (observe !== undefined && typeof observe !== "function")) {
    throw new Error("operation_invalid_host_options");
  }
  const operations = registry.describe();
  const capabilities = new Map(operations.map((op) => [op.name, op.capability]));
  const caps = new Set(capabilities.values());
  const endpoint = Object.freeze({ window: "operations", epoch: randomUUID() });
  let attempt: Attempt | undefined;
  let completion: Completion<P> | undefined;
  const lifetime = new AbortController();
  let busy = false;
  let serial = 0;
  const carrier = createWorkerTransport(options, {
    profile,
    operations,
    grants: () => [...attempt!.grants],
    retire: () => attempt?.effects.abort(),
  });

  function currentGrants(): ReadonlySet<string> {
    const permitted = grants();
    if (!Array.isArray(permitted) || permitted.length > caps.size) throw new Error("operation_invalid_grants");
    const unique = new Set<string>();
    for (const cap of permitted) {
      if (!caps.has(cap) || unique.has(cap)) throw new Error("operation_invalid_grants");
      unique.add(cap);
    }
    return unique;
  }

  function available(signal: AbortSignal): void {
    if (lifetime.signal.aborted || signal.aborted) throw new Error("operation_host_closed");
    if (busy) throw new Error("operation_host_busy");
  }

  const snapshot = () => observe && json(observe(), "operation_observation");

  return {
    async describe(signal) {
      available(signal);
      completion = undefined;
      return carrier.describe(signal);
    },
    async execute(candidate, signal) {
      available(signal);
      busy = true;
      completion = undefined;
      const effects = new AbortController();
      try {
        const plan = proposal(candidate);
        const before = snapshot();
        const admitted = currentGrants();
        if (lifetime.signal.aborted || signal.aborted) throw new Error("operation_host_closed");
        if (serial === Number.MAX_SAFE_INTEGER) throw new Error("operation_session_exhausted");
        const scope = { ...endpoint, session: `run_${++serial}` };
        let delivered = 0;
        let refused = false;
        attempt = { effects, grants: admitted };

        // The carrier checks scope, serial, operation name and challenge before delivery.
        async function dispatch(request: { operation: string; arguments: unknown; request_id: number; budget_ms: number }) {
          const deadline = performance.now() + request.budget_ms;
          const actionSignal = AbortSignal.any([signal, effects.signal, AbortSignal.timeout(request.budget_ms)]);
          const allows = (cap: string) => admitted.has(cap) && currentGrants().has(cap)
            && !actionSignal.aborted && performance.now() < deadline;
          const identity = { transport_version: 1 as const, ...scope, request_id: request.request_id };
          let value: OperationWireValue;
          try {
            value = await registry.dispatch(request.operation, request.arguments, { signal: actionSignal, allows });
            if (!allows(capabilities.get(request.operation)!)) throw new Error("operation_authority_lost");
          } catch {
            if (effects.signal.aborted || signal.aborted) return;
            refused = true;
            effects.abort();
            await carrier.reply({ ...identity, status: "uncertain", value: null, error: "operation_failed" });
            return;
          }
          delivered++;
          await carrier.reply({ ...identity, status: "ok", value, error: null });
        }

        const raw = await carrier.execute({ scope, source: plan.source, arguments: plan.arguments,
          inputMode: plan.inputMode, onRequest(request) {
            if (refused) throw new Error("operation_after_failure");
            void dispatch(request as Parameters<typeof dispatch>[0]).catch(() => carrier.cancel(scope));
          } }, signal);
        if (lifetime.signal.aborted || signal.aborted) throw new Error("operation_host_closed");
        if (refused) throw new Error("operation_after_failure");
        const report = admitReport(raw, delivered, profile);
        completion = { plan, raw, report, signal, before };
        return raw;
      } finally {
        effects.abort();
        attempt = undefined;
        busy = false;
      }
    },
    async verify(raw, candidate, signal) {
      if (busy || lifetime.signal.aborted || signal.aborted) return false;
      const evidence = completion;
      // Even identical source, arguments and result bytes may belong to an older submission.
      if (!evidence || signal !== evidence.signal) return false;
      completion = undefined;
      if (raw !== evidence.raw) return false;
      busy = true;
      try {
        const plan = proposal(candidate);
        if (!samePlan(plan, evidence.plan)) return false;
        const verificationSignal = AbortSignal.any([signal, lifetime.signal]);
        const after = snapshot();
        if (verificationSignal.aborted) return false;
        const verified = await verify({ plan: evidence.plan, report: evidence.report,
          before: evidence.before, after, signal: verificationSignal });
        return verified === true && !verificationSignal.aborted;
      } finally { busy = false; }
    },
    close() {
      lifetime.abort();
      completion = undefined;
      attempt?.effects.abort();
      carrier.close(endpoint);
    },
  };
}
