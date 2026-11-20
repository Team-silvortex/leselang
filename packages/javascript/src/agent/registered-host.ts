import { Bridge, type Reply, type Scope } from "../bridge.js";
import { prepareSemanticCatalog, type PreparedSemanticTask, type SemanticCatalog } from "../semantics/index.js";
import { fault, json, proposal } from "./data.js";
import { observations, samePlan, snapshots } from "./registered-evidence.js";
import type { AgentHost } from "./session.js";
import { VERIFICATION_SUCCESS, rejectVerification, type VerificationResult } from "../verification.js";

/** The application retains ownership of its objects and independent evidence. */
export interface RegisteredApplicationConnection {
  attach(bridge: Bridge): void;
  detach(bridge: Bridge): void;
  observe(): unknown;
}

export interface RegisteredAgentEndpoint {
  readonly window: string;
  readonly epoch: string;
}

export interface RegisteredAgentRun {
  readonly scope: Readonly<Scope>;
  readonly source: string;
  readonly arguments: Readonly<Record<string, string>>;
  readonly inputMode?: "named_strings";
  readonly onRequest: (request: unknown) => void;
}

/** Transport owns compilation, explicit VM driving and actual settlement. */
export interface RegisteredAgentTransport {
  describe(signal: AbortSignal): Promise<unknown>;
  execute(run: RegisteredAgentRun, signal: AbortSignal): Promise<unknown>;
  reply(reply: Reply): void | Promise<void>;
  cancel(scope: Readonly<Scope>): void | Promise<void>;
  close(endpoint: RegisteredAgentEndpoint): void | Promise<void>;
}

export interface RegisteredAgentHostOptions extends RegisteredAgentEndpoint {
  readonly application: RegisteredApplicationConnection;
  readonly transport: RegisteredAgentTransport;
  /** Distinct top-level report field names, chosen by the trusted application. */
  readonly observations: { readonly before: string; readonly after: string };
  /** A throwing callback refuses execution; it cannot authorize another plan. */
  readonly onSource?: (source: string) => void;
}

export interface RegisteredAgentHost extends AgentHost {
  verifyDetailed: NonNullable<AgentHost["verifyDetailed"]>;
  /** Fence this endpoint immediately; outstanding transport work still settles. */
  close(): void;
}

interface Slot { stop(): void }
interface Completion {
  readonly prepared: PreparedSemanticTask;
  readonly before: string;
  readonly report: string | null;
  readonly verification: VerificationResult;
  readonly run: Slot;
  live(): boolean;
}

/**
 * An optional registered-object host above the existing AgentHost and Bridge.
 * One host exclusively owns a globally fresh window/epoch endpoint. Sessions
 * increase monotonically within it; replacing a host requires a fresh epoch.
 * No VM loop, scheduler, automatic retry or rollback is installed here.
 */
export function createRegisteredAgentHost(options: RegisteredAgentHostOptions): RegisteredAgentHost {
  if (!options || typeof options !== "object"
    || !options.application || !options.transport
    || [options.application.attach, options.application.detach, options.application.observe,
      options.transport.describe, options.transport.execute, options.transport.reply,
      options.transport.cancel, options.transport.close].some((method) => typeof method !== "function")
    || (options.onSource !== undefined && typeof options.onSource !== "function")) {
    fault("invalid_registered_host_options");
  }
  let endpoint: RegisteredAgentEndpoint;
  try {
    const probe = new Bridge({ window: options.window, epoch: options.epoch, session: "validation" });
    endpoint = Object.freeze({ window: probe.scope.window, epoch: probe.scope.epoch });
    probe.close();
  } catch { fault("invalid_registered_endpoint"); }
  const fieldName = (value: unknown): value is string => typeof value === "string"
    && /^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(value);
  const selected = options.observations;
  if (!selected || !fieldName(selected.before) || !fieldName(selected.after) || selected.before === selected.after) {
    fault("invalid_registered_observation_fields");
  }
  const observationFields = Object.freeze({ before: selected.before, after: selected.after });
  const { application, transport, onSource } = options;
  let active: Slot | null = null;
  let closed = false;
  let catalog: SemanticCatalog | null = null;
  let completed: Completion | null = null;
  let lastSession = 0;

  function check(signal: AbortSignal): void {
    if (closed) fault("registered_host_closed");
    if (signal.aborted) fault("registered_host_cancelled");
  }
  function available(signal: AbortSignal): void {
    check(signal);
    if (active) fault("registered_host_busy");
  }
  function quietly(callback: () => void | Promise<void>): void {
    try { void Promise.resolve(callback()).catch(() => {}); }
    catch { /* Local fences never depend on a transport acknowledgement. */ }
  }

  function verifyDetailed(report: unknown, plan: Parameters<AgentHost["verify"]>[1], signal: AbortSignal): VerificationResult {
    if (signal.aborted) fault("registered_host_cancelled");
    if (closed) return rejectVerification("verification_interrupted");
    if (active) return rejectVerification("completion_unavailable");
    const evidence = completed;
    completed = null;
    if (!evidence) return rejectVerification("completion_unavailable");
    if (!evidence.live()) return rejectVerification("verification_interrupted");
    const current: Slot = { stop() {} };
    active = current;
    const live = () => !closed && !signal.aborted && evidence.live() && active === current;
    try {
      try {
        if (!samePlan(proposal(plan), evidence.prepared.plan)) return rejectVerification("plan_mismatch");
      } catch { return rejectVerification("plan_mismatch"); }
      // Bind every explanation to this attempt before exposing semantic detail.
      // Non-string/oversized transport results are never retained as evidence.
      if (typeof report !== "string" || evidence.report === null) return rejectVerification("invalid_report");
      if (report !== evidence.report) return rejectVerification("report_mismatch");
      if (!live()) return rejectVerification("verification_interrupted");
      if (!evidence.verification.verified) return evidence.verification;
      const observed = observations(report, observationFields);
      const after = snapshots(application.observe());
      if (!live()) return rejectVerification("verification_interrupted");
      if (observed.before !== evidence.before) return rejectVerification("before_observation_mismatch");
      if (observed.after !== after) return rejectVerification("after_observation_mismatch");
      return VERIFICATION_SUCCESS;
    } catch {
      return rejectVerification(live() ? "observation_invalid" : "verification_interrupted");
    } finally { if (active === current) active = null; }
  }

  return {
    async describe(signal) {
      available(signal);
      const controller = new AbortController();
      const current: Slot = { stop: () => controller.abort() };
      active = current;
      catalog = null;
      completed = null;
      signal.addEventListener("abort", current.stop, { once: true });
      try {
        check(signal);
        const described = json(await transport.describe(controller.signal), "catalog", 256 * 1024);
        check(signal);
        if (active !== current || controller.signal.aborted) fault("registered_host_cancelled");
        const prepared = prepareSemanticCatalog(described);
        check(signal);
        if (active !== current || controller.signal.aborted) fault("registered_host_cancelled");
        catalog = prepared;
        return described;
      } finally {
        signal.removeEventListener("abort", current.stop);
        if (active === current) active = null;
      }
    },

    async execute(plan, signal) {
      available(signal);
      if (!catalog) fault("registered_catalog_unavailable");
      if (lastSession === 0xffff_ffff) fault("registered_session_exhausted");
      const selectedCatalog = catalog;
      const bridge = new Bridge({ ...endpoint, session: `run_${++lastSession}` });
      const controller = new AbortController();
      let stopped = false;
      let started = false;
      let attaching = false;
      let attachmentStarted = false;
      let detached = false;
      let cleanupFailed = false;
      let cleanupError: unknown;
      const detach = () => {
        if (!attachmentStarted || attaching || detached) return;
        detached = true;
        try { application.detach(bridge); }
        catch (error) { cleanupFailed = true; cleanupError = error; throw error; }
      };
      const current: Slot = {
        stop() {
          if (stopped) return;
          stopped = true;
          if (completed?.run === current) completed = null;
          bridge.close();
          controller.abort();
          try { detach(); } catch { /* Cleanup failure is retained for final settlement. */ }
          // Even a late delivery failure belongs to this captured scope. A
          // transport may still need to retire its delivery state after execute.
          if (started) quietly(() => transport.cancel(bridge.scope));
        },
      };
      const live = () => !closed && !stopped && !signal.aborted && !controller.signal.aborted;
      const checkRun = () => {
        check(signal);
        if (!live() || active !== current) fault("registered_host_cancelled");
      };
      // Claim the slot and cancellation fence before any application callback,
      // including observe() and partially successful attach().
      active = current;
      completed = null;
      signal.addEventListener("abort", current.stop, { once: true });
      let report: unknown;
      let candidate: Completion | null = null;
      try {
        checkRun();
        const ownedPlan = proposal(plan);
        checkRun();
        if (!ownedPlan.task) fault("registered_plan_task_required");
        const prepared = selectedCatalog.prepare(ownedPlan.task, ownedPlan.arguments);
        if (!samePlan(prepared.plan, ownedPlan)) fault("registered_plan_mismatch");
        checkRun();
        const before = snapshots(application.observe());
        checkRun();
        attachmentStarted = true;
        attaching = true;
        try { application.attach(bridge); } finally { attaching = false; }
        checkRun();
        onSource?.(prepared.plan.source);
        checkRun();
        const run: RegisteredAgentRun = Object.freeze({
          scope: bridge.scope, source: prepared.plan.source, arguments: prepared.plan.arguments,
          ...(prepared.plan.inputMode ? { inputMode: prepared.plan.inputMode } : {}),
          onRequest(request: unknown) {
            // This closure always retains its own bridge and stop function.
            // Late requests receive rejection without affecting a later run.
            void bridge.handle(request).then((reply) => transport.reply(reply)).catch(current.stop);
          },
        });
        started = true;
        report = await transport.execute(run, controller.signal);
        if (live()) {
          const raw = typeof report === "string" && report.length <= 128 * 1024
            && new TextEncoder().encode(report).length <= 128 * 1024 ? report : null;
          const verification = raw === null ? rejectVerification("invalid_report") : prepared.verifyDetailed(raw);
          candidate = { prepared, before, report: raw, verification, run: current, live };
        }
      } finally {
        bridge.close();
        try {
          detach();
          if (cleanupFailed) throw cleanupError;
        } finally {
          signal.removeEventListener("abort", current.stop);
          if (active === current) active = null;
        }
      }
      // Publish evidence only after successful teardown and all callbacks. A
      // reentrant verify during detach cannot accept an unfinished execution.
      if (candidate && live()) completed = candidate;
      return report;
    },

    verify(report, plan, signal) {
      return verifyDetailed(report, plan, signal).verified;
    },
    verifyDetailed,

    close() {
      if (closed) return;
      closed = true;
      catalog = null;
      completed = null;
      active?.stop();
      quietly(() => transport.close(endpoint));
    },
  };
}
