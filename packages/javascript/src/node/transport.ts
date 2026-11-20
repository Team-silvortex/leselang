import { Buffer } from "node:buffer";
import { spawn } from "node:child_process";
import { isAbsolute } from "node:path";
import { clearTimeout, setTimeout } from "node:timers";
import type { Reply, Scope } from "../bridge.js";
import type { RegisteredAgentEndpoint, RegisteredAgentRun, RegisteredAgentTransport } from "../agent/registered-host.js";

/** A trusted executable implementing the registered-agent worker protocol. */
export interface NodeAgentTransportOptions {
  /** Absolute path chosen by the host, never by an agent-generated plan. */
  readonly executable: string;
  /** Fixed arguments preceding --describe or --serve; no shell is invoked. */
  readonly args?: readonly string[];
  /** Add the application's declared semantics to the worker's base raw catalog. */
  readonly catalog: (baseCatalog: string) => string;
  /** Opt into worker protocol 3 execution failure reports; enable the host's matching report contract too. */
  readonly executionReports?: "v1";
  /** Whole-child deadline, including clean process closure: 20..60000 ms (default 15000). */
  readonly timeoutMs?: number;
  /** Display-only diagnostics; callback exceptions cannot authorize execution. */
  readonly onStderr?: (text: string) => void;
}

interface ActiveWorker {
  readonly scope: Readonly<Scope> | undefined;
  fail(reason: unknown): void;
  reply(reply: Reply): Promise<void>;
}
type CapturedRun = Pick<RegisteredAgentRun, "scope" | "onRequest"> & { readonly version: 1 | 2 | 3 };

const START_LIMIT = 128 * 1024;
const REQUEST_LIMIT = 64 * 1024;
const REPORT_LIMIT = 128 * 1024;
const FRAME_LIMIT = 512 * 1024;
const CATALOG_LIMIT = 256 * 1024;
const STDERR_LIMIT = 128 * 1024;
const OUTPUT_LIMIT = 256 * REQUEST_LIMIT + FRAME_LIMIT;
const VERSION = 1;
const failure = (code: string): Error => new Error(code);
const label = (value: unknown): value is string => typeof value === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(value);

function validText(value: unknown, limit: number): value is string {
  if (typeof value !== "string" || Buffer.byteLength(value) > limit) return false;
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) return false;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) return false;
  }
  return true;
}

/** Strict bounded envelope JSON. Report JSON stays an opaque original string. */
function decode(bytes: Uint8Array): unknown {
  const source = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  let offset = 0, nodes = 0;
  const invalid = (): never => { throw failure("node_worker_invalid_json"); };
  const whitespace = () => { while (/[\x20\t\r\n]/.test(source[offset] ?? "x")) offset++; };
  const string = (): string => {
    if (source[offset] !== '"') invalid();
    const start = offset++;
    while (offset < source.length) {
      const unit = source[offset++];
      if (unit === "\\") offset++;
      else if (unit === '"') {
        const value = JSON.parse(source.slice(start, offset));
        if (!validText(value, FRAME_LIMIT)) invalid();
        return value;
      }
    }
    return invalid();
  };
  const value = (depth: number): unknown => {
    if (++nodes > 16_384 || depth > 32) invalid();
    whitespace();
    const token = source[offset];
    if (token === '"') return string();
    if (token === "{") {
      offset++; whitespace();
      const object: Record<string, unknown> = Object.create(null);
      if (source[offset] === "}") { offset++; return object; }
      for (;;) {
        whitespace();
        const key = string();
        if (Object.hasOwn(object, key)) invalid();
        whitespace();
        if (source[offset++] !== ":") invalid();
        object[key] = value(depth + 1);
        whitespace();
        const next = source[offset++];
        if (next === "}") return object;
        if (next !== ",") invalid();
      }
    }
    if (token === "[") {
      offset++; whitespace();
      const array: unknown[] = [];
      if (source[offset] === "]") { offset++; return array; }
      for (;;) {
        array.push(value(depth + 1)); whitespace();
        const next = source[offset++];
        if (next === "]") return array;
        if (next !== ",") invalid();
      }
    }
    for (const [word, result] of [["null", null], ["true", true], ["false", false]] as const) {
      if (source.startsWith(word, offset)) { offset += word.length; return result; }
    }
    const number = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(source.slice(offset));
    if (!number) return invalid();
    offset += number[0].length;
    const result = Number(number[0]);
    if (!Number.isSafeInteger(result)) invalid();
    return result;
  };
  const result = value(0);
  whitespace();
  if (offset !== source.length) invalid();
  return result;
}

function exact(record: unknown, keys: readonly string[]): asserts record is Record<string, unknown> {
  if (!record || typeof record !== "object" || Array.isArray(record)
    || Object.keys(record).length !== keys.length || keys.some((key) => !Object.hasOwn(record, key))) {
    throw failure("node_worker_invalid_frame");
  }
}
const record = (value: unknown): value is Record<string, unknown> => !!value && typeof value === "object" && !Array.isArray(value);
const sameEndpoint = (a: unknown, b: unknown): boolean => record(a) && record(b) && a.window === b.window && a.epoch === b.epoch;
const sameScope = (a: unknown, b: unknown): boolean => record(a) && record(b) && sameEndpoint(a, b) && a.session === b.session;

/**
 * One fresh worker per task. Application state remains in the parent process.
 * A report never completes execute until the child has actually closed cleanly.
 */
export function createNodeAgentTransport(options: NodeAgentTransportOptions): RegisteredAgentTransport {
  if (!options || typeof options !== "object" || !validText(options.executable, 4096) || !isAbsolute(options.executable) || options.executable.includes("\0")
    || typeof options.catalog !== "function"
    || (options.args !== undefined && (!Array.isArray(options.args)
      || options.args.length > 16 || options.args.some((arg) => !validText(arg, 4096) || arg.includes("\0"))))
    || (options.onStderr !== undefined && typeof options.onStderr !== "function")) {
    throw failure("node_worker_invalid_options");
  }
  const executionReports = options.executionReports;
  if (executionReports !== undefined && executionReports !== "v1") throw failure("node_worker_invalid_options");
  const timeoutMs = options.timeoutMs ?? 15_000;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs < 20 || timeoutMs > 60_000) throw failure("node_worker_invalid_timeout");
  const executable = options.executable, args = [...(options.args ?? [])];
  const catalog = options.catalog, onStderr = options.onStderr;
  let active: ActiveWorker | null = null;
  let closed = false;
  let endpoint: RegisteredAgentEndpoint | null = null;

  function available(signal: AbortSignal): void {
    if (closed) throw failure("node_worker_closed");
    if (!signal || typeof signal.aborted !== "boolean" || typeof signal.addEventListener !== "function") {
      throw failure("node_worker_invalid_signal");
    }
    if (signal.aborted) throw failure("node_worker_cancelled");
    if (active) throw failure("node_worker_busy");
  }

  function launch(mode: "--describe" | "--serve", signal: AbortSignal, run?: CapturedRun, first?: Uint8Array): Promise<string> {
    available(signal);
    const version = run?.version ?? VERSION;
    return new Promise<string>((resolve, reject) => {
      const child = spawn(executable, [...args, mode], { shell: false, stdio: ["pipe", "pipe", "pipe"] });
      let error: Error | null = null;
      let forceKill: ReturnType<typeof setTimeout> | undefined;
      let timer: ReturnType<typeof setTimeout> | undefined;
      let terminal = false, report: string | undefined;
      let pending: { readonly challenge: string; readonly serial: number } | null = null;
      let stdoutBytes = 0, stderrBytes = 0, lastSerial = 0, buffer = Buffer.alloc(0);
      const stderrDecoder = new TextDecoder("utf-8", { fatal: true });
      const current: ActiveWorker = {
        scope: run?.scope,
        fail(reason) {
          if (error) return;
          error = reason instanceof Error ? reason : failure("node_worker_failed");
          pending = null;
          child.stdin.destroy();
          try { child.kill("SIGTERM"); } catch { /* The close event owns settlement. */ }
          forceKill = setTimeout(() => { try { child.kill("SIGKILL"); } catch {} }, 100);
        },
        reply(reply) {
          if (error || !pending) throw failure("node_worker_unexpected_reply");
          exact(reply, ["transport_version", "window", "epoch", "session", "request_id", "status", "value", "error"]);
          if (!sameScope(reply, current.scope) || reply.transport_version !== 1 || reply.request_id !== pending.serial) {
            throw failure("node_worker_foreign_reply");
          }
          const bytes = Buffer.from(JSON.stringify({ agent_worker_version: version, kind: "reply",
            challenge: pending.challenge, reply }) + "\n");
          if (bytes.length > REQUEST_LIMIT) throw failure("node_worker_reply_limit");
          // Retire this delivery before callbacks or the next worker request.
          pending = null;
          return new Promise<void>((yes, no) => child.stdin.write(bytes, (writeError) => {
            if (writeError) { current.fail(failure("node_worker_write_failed")); no(writeError); }
            else yes();
          }));
        },
      };
      active = current;
      const abort = () => current.fail(failure("node_worker_cancelled"));
      signal.addEventListener("abort", abort, { once: true });
      timer = setTimeout(() => current.fail(failure("node_worker_deadline")), timeoutMs);
      const frame = (bytes: Uint8Array): void => {
        if (terminal) throw failure("node_worker_after_terminal");
        const message = decode(bytes);
        if (!record(message) || message.agent_worker_version !== version) throw failure("node_worker_invalid_version");
        if (message.kind === "request") {
          exact(message, ["agent_worker_version", "kind", "challenge", "request"]);
          if (bytes.length > REQUEST_LIMIT || pending || typeof message.challenge !== "string"
            || !/^[0-9a-f]{64}$/.test(message.challenge)) {
            throw failure("node_worker_invalid_request");
          }
          const request = message.request;
          exact(request, ["transport_version", "window", "epoch", "session", "request_id", "budget_ms", "operation", "arguments"]);
          if (!run || !sameScope(request, run.scope) || request.transport_version !== 1
            || typeof request.request_id !== "number" || request.request_id !== lastSerial + 1 || request.request_id > 256
            || typeof request.budget_ms !== "number" || !Number.isInteger(request.budget_ms) || request.budget_ms < 1 || request.budget_ms > 5000
            || typeof request.operation !== "string" || !["gui.list", "gui.find", "gui.observe", "gui.set_text", "gui.click"].includes(request.operation)
            || !request.arguments || typeof request.arguments !== "object" || Array.isArray(request.arguments)) {
            throw failure("node_worker_foreign_request");
          }
          lastSerial = request.request_id;
          pending = { challenge: message.challenge, serial: lastSerial };
          run.onRequest(request);
        } else if (message.kind === "report") {
          exact(message, ["agent_worker_version", "kind", "report_json"]);
          if (pending || !validText(message.report_json, REPORT_LIMIT) || !message.report_json) throw failure("node_worker_invalid_report");
          terminal = true;
          report = message.report_json;
        } else if (message.kind === "error") {
          exact(message, ["agent_worker_version", "kind", "error"]);
          if (!validText(message.error, 1024) || !message.error) throw failure("node_worker_invalid_error");
          terminal = true;
          throw failure(`node_worker_failed: ${message.error}`);
        } else throw failure("node_worker_unknown_frame");
      };
      child.stdout.on("data", (chunk) => {
        if (error) return;
        try {
          stdoutBytes += chunk.length;
          if (stdoutBytes > (mode === "--describe" ? CATALOG_LIMIT : OUTPUT_LIMIT)) throw failure("node_worker_output_limit");
          if (mode === "--describe") { buffer = Buffer.concat([buffer, chunk]); return; }
          if (terminal && chunk.length) throw failure("node_worker_after_terminal");
          let start = 0;
          for (let end = 0; end < chunk.length; end++) {
            if (chunk[end] !== 10) continue;
            const part = chunk.subarray(start, end + 1);
            if (buffer.length + part.length > FRAME_LIMIT) throw failure("node_worker_frame_limit");
            const line = Buffer.concat([buffer, part]); buffer = Buffer.alloc(0);
            frame(line);
            start = end + 1;
          }
          const remainder = chunk.subarray(start);
          if (terminal && remainder.length) throw failure("node_worker_after_terminal");
          if (buffer.length + remainder.length > FRAME_LIMIT) throw failure("node_worker_frame_limit");
          buffer = Buffer.concat([buffer, remainder]);
        } catch (caught) { current.fail(caught); }
      });
      child.stderr.on("data", (chunk) => {
        if (error) return;
        try {
          stderrBytes += chunk.length;
          if (stderrBytes > STDERR_LIMIT) throw failure("node_worker_stderr_limit");
          const text = stderrDecoder.decode(chunk, { stream: true });
          try { onStderr?.(text); } catch { /* Diagnostics never grant authority. */ }
        } catch (caught) { current.fail(caught); }
      });
      child.stdin.on("error", () => current.fail(failure("node_worker_write_failed")));
      child.stdout.on("error", () => current.fail(failure("node_worker_read_failed")));
      child.stderr.on("error", () => current.fail(failure("node_worker_read_failed")));
      child.on("error", () => current.fail(failure("node_worker_spawn_failed")));
      child.on("close", (code, exitSignal) => {
        clearTimeout(timer); clearTimeout(forceKill);
        signal.removeEventListener("abort", abort);
        if (active === current) active = null;
        try {
          if (error) throw error;
          if (signal.aborted || closed) throw failure("node_worker_cancelled");
          if (exitSignal || code !== 0) throw failure("node_worker_exit_failed");
          stderrDecoder.decode();
          if (mode === "--describe") {
            decode(buffer);
            resolve(new TextDecoder("utf-8", { fatal: true }).decode(buffer));
          } else {
            if (pending || buffer.length || !terminal || typeof report !== "string") throw failure("node_worker_incomplete_result");
            resolve(report);
          }
        } catch (caught) { reject(caught); }
      });
      if (signal.aborted) abort();
      if (!error && first) child.stdin.write(first, (writeError) => {
        if (writeError) current.fail(failure("node_worker_write_failed"));
      });
    });
  }

  return {
    async describe(signal) {
      const base = await launch("--describe", signal);
      if (closed || signal.aborted) throw failure("node_worker_cancelled");
      const described = catalog(base);
      if (closed || signal.aborted) throw failure("node_worker_cancelled");
      if (!validText(described, CATALOG_LIMIT)) throw failure("node_worker_invalid_catalog");
      return described;
    },
    async execute(run, signal) {
      available(signal);
      if (!run) throw failure("node_worker_invalid_scope");
      const selected = run.scope, onRequest = run.onRequest;
      const scope = Object.freeze({ window: selected?.window, epoch: selected?.epoch, session: selected?.session });
      if (typeof onRequest !== "function" || !label(scope.window)
        || !label(scope.epoch) || !label(scope.session)) throw failure("node_worker_invalid_scope");
      if (endpoint && !sameEndpoint(endpoint, scope)) throw failure("node_worker_foreign_endpoint");
      const source = run.source, inputMode = run.inputMode, supplied = run.arguments;
      if (!validText(source, 16 * 1024) || source.includes("\0")) throw failure("node_worker_invalid_source");
      if (inputMode !== undefined && inputMode !== "named_strings") throw failure("node_worker_invalid_input_mode");
      const namedInputs = inputMode === "named_strings";
      const version = executionReports === "v1" ? 3 : namedInputs ? 2 : 1;
      if (!supplied || typeof supplied !== "object" || Array.isArray(supplied)
        || ![Object.prototype, null].includes(Object.getPrototypeOf(supplied))) {
        throw failure("node_worker_invalid_arguments");
      }
      const keys = Reflect.ownKeys(supplied);
      if (keys.length > (namedInputs ? 8 : 1)) throw failure("node_worker_invalid_arguments");
      const arguments_: Record<string, string> = Object.create(null);
      let total = 0;
      for (const name of keys) {
        const field = Object.getOwnPropertyDescriptor(supplied, name);
        if (typeof name !== "string" || !/^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(name)
          || ["fn", "true", "false", "none", "body"].includes(name) || name.startsWith("_lese_")
          || !field?.enumerable || !("value" in field) || !validText(field.value, 4096) || field.value.includes("\0")) {
          throw failure("node_worker_invalid_arguments");
        }
        total += Buffer.byteLength(field.value);
        if (total > 16 * 1024) throw failure("node_worker_invalid_arguments");
        arguments_[name] = field.value;
      }
      const captured: CapturedRun = { scope, onRequest, version };
      const bytes = Buffer.from(JSON.stringify({ agent_worker_version: version, kind: "start",
        scope, source, arguments: arguments_,
        ...(version === 3 ? { input_mode: namedInputs ? "named_strings" : "single_string" } : {}) }) + "\n");
      if (bytes.length > START_LIMIT) throw failure("node_worker_start_limit");
      endpoint ??= Object.freeze({ window: scope.window, epoch: scope.epoch });
      return launch("--serve", signal, captured, bytes);
    },
    reply(reply) {
      // A late old reply must not retire the currently running new child.
      if (!active?.scope || !sameScope(active.scope, reply)) return Promise.reject(failure("node_worker_foreign_reply"));
      const current = active;
      try { return current.reply(reply); }
      catch (error) { current.fail(error); return Promise.reject(error); }
    },
    cancel(scope) {
      if (active?.scope && sameScope(active.scope, scope)) active.fail(failure("node_worker_cancelled"));
    },
    close(selected) {
      if (endpoint && !sameEndpoint(endpoint, selected)) return;
      if (closed) return;
      closed = true;
      active?.fail(failure("node_worker_closed"));
    },
  };
}
