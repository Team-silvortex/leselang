import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { createNodeAgentTransport } from "../dist/node/index.js";

const worker = fileURLToPath(new URL("./fixtures/node-worker.mjs", import.meta.url));
const scope = (session = "run_1") => ({ window: "node_app", epoch: "fresh_epoch", session });
const signal = () => new AbortController().signal;
const expected = '{"integer":18446744073709551615}';
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
function fixture(t, mode = "normal", options = {}) {
  const carrier = createNodeAgentTransport({ executable: process.execPath, args: [worker, mode],
    catalog: (raw) => raw, timeoutMs: 3000, ...options });
  t.after(() => carrier.close(scope()));
  const received = [];
  const reply = (request) => ({ transport_version: 1, window: request.window, epoch: request.epoch,
    session: request.session, request_id: request.request_id, status: "ok", value: [], error: null });
  const run = (session = "run_1", handler) => ({ scope: scope(session), source: "fn main() = 1", arguments: {},
    onRequest(request) {
      received.push(request);
      if (handler) handler(request);
      else void carrier.reply(reply(request)).catch(() => {});
    },
  });
  return { carrier, run, received, reply };
}

test("a fresh Node-owned worker preserves raw integer report bytes and scoped replies", async (t) => {
  const f = fixture(t);
  const described = await f.carrier.describe(signal());
  assert.equal(JSON.parse(described).profile, "javascript.registered");
  assert.equal(await f.carrier.execute(f.run(), signal()), expected);
  assert.equal(await f.carrier.execute(f.run("run_2"), signal()), expected);
  assert.deepEqual(f.received.map((item) => item.session), ["run_1", "run_2"]);
});

test("an explicit named-input run pins protocol two without changing adjacent legacy runs", async (t) => {
  const f = fixture(t, "echo_arguments");
  const first = JSON.parse(await f.carrier.execute(f.run(), signal()));
  assert.equal(first.version, 1);
  const arguments_ = { title: 'Quoted "标题"', content: "literal\\path\n正文 ✓" };
  const next = { ...f.run("run_2"), inputMode: "named_strings", source: "fn main(title: string, content: string) = title", arguments: arguments_ };
  const pending = f.carrier.execute(next, signal());
  arguments_.title = "later mutation";
  next.inputMode = undefined;
  next.source = "later source";
  const second = JSON.parse(await pending);
  assert.equal(second.version, 2);
  assert.equal(second.source, "fn main(title: string, content: string) = title");
  assert.deepEqual(second.arguments, { title: 'Quoted "标题"', content: "literal\\path\n正文 ✓" });
  assert.equal(JSON.parse(await f.carrier.execute(f.run("run_3"), signal())).version, 1);
});

test("named input bounds and markers reject before dispatch and never evaluate argument getters", async (t) => {
  const f = fixture(t);
  let getters = 0;
  const getter = { title: "ok" };
  Object.defineProperty(getter, "content", { enumerable: true, get() { getters++; return "not data"; } });
  for (const change of [
    { arguments: { title: "one", content: "two" } },
    { inputMode: "unknown", arguments: {} },
    { inputMode: null, arguments: {} },
    { inputMode: "named_strings", arguments: getter },
    { inputMode: "named_strings", arguments: { title: "valid", content: true } },
    { inputMode: "named_strings", arguments: { title: "valid", content: "\ud800" } },
    { inputMode: "named_strings", arguments: { title: "valid", content: "nul\0data" } },
    { inputMode: "named_strings", arguments: { title: "valid", content: "x".repeat(4097) } },
    { inputMode: "named_strings", arguments: Object.fromEntries(Array.from({ length: 9 }, (_, i) => [`p${i}`, "x"])) },
    { inputMode: "named_strings", arguments: Object.fromEntries(Array.from({ length: 8 }, (_, i) => [`p${i}`, "x".repeat(2049)])) },
  ]) await assert.rejects(f.carrier.execute({ ...f.run(), ...change }, signal()), /invalid_(arguments|input_mode)/);
  assert.equal(getters, 0);
  assert.equal(f.received.length, 0);
  assert.equal(await f.carrier.execute({ ...f.run(), inputMode: "named_strings",
    arguments: Object.fromEntries(Array.from({ length: 8 }, (_, i) => [`p${i}`, "x".repeat(2048)])) }, signal()), expected);
});

test("a worker cannot upgrade or downgrade the selected startup protocol", async (t) => {
  for (const [mode, inputMode] of [["downgrade", "named_strings"], ["upgrade", undefined]]) {
    const f = fixture(t, mode);
    await assert.rejects(f.carrier.execute({ ...f.run(), ...(inputMode ? { inputMode } : {}) }, signal()), /invalid_version/);
    assert.equal(f.received.length, 0);
  }
});

test("a terminal report does not release the process slot before clean child closure", async (t) => {
  const ready = deferred();
  const f = fixture(t, "report_held", { onStderr: (text) => { if (text.includes("report_sent")) ready.resolve(); } });
  let settled = false;
  const pending = f.carrier.execute(f.run(), signal()).finally(() => { settled = true; });
  await ready.promise;
  assert.equal(settled, false);
  await assert.rejects(f.carrier.execute(f.run("run_2"), signal()), /busy/);
  await assert.rejects(f.carrier.describe(signal()), /busy/);
  assert.equal(await pending, expected);
  assert.equal(settled, true);
});

test("terminal failures, malformed frames and truncation never become successful reports", async (t) => {
  for (const mode of ["report_nonzero", "duplicate_terminal", "truncated", "invalid_utf8", "duplicate_key",
    "nested_duplicate", "extra_key", "unsafe_integer", "invalid_surrogate", "oversized", "report_oversized",
    "stderr_oversized", "no_report", "error", "foreign_scope", "serial_gap", "request_double", "pending_report",
    "after_report", "bad_challenge"]) {
    const f = fixture(t, mode);
    await assert.rejects(f.carrier.execute(f.run("run_1", mode === "pending_report" ? () => {} : undefined), signal()), undefined, mode);
  }
});

test("descriptions are bounded, strict UTF-8 and duplicate-free before catalog attachment", async (t) => {
  for (const mode of ["description_duplicate", "description_oversized", "description_invalid_utf8"]) {
    let calls = 0;
    const f = fixture(t, mode, { catalog(raw) { calls++; return raw; } });
    await assert.rejects(f.carrier.describe(signal()));
    assert.equal(calls, 0, mode);
  }
});

test("challenge validation never coerces an array before application dispatch", async (t) => {
  const f = fixture(t, "array_challenge");
  await assert.rejects(f.carrier.execute(f.run(), signal()), /invalid_request/);
  assert.equal(f.received.length, 0);
});

test("a worker that reports but stays alive fails the overall deadline", async (t) => {
  const f = fixture(t, "report_hang", { timeoutMs: 150 });
  await assert.rejects(f.carrier.execute(f.run(), signal()), /deadline/);
});

test("cancellation retains the slot until a SIGTERM-resistant child is killed and reaped", async (t) => {
  const ready = deferred();
  let pid;
  const f = fixture(t, "ignore_term", { onStderr(text) {
    const match = /ready:(\d+)/.exec(text);
    if (match) { pid = Number(match[1]); ready.resolve(); }
  } });
  const controller = new AbortController();
  const pending = f.carrier.execute(f.run(), controller.signal);
  const rejected = assert.rejects(pending, /cancelled/);
  await ready.promise;
  controller.abort();
  await assert.rejects(f.carrier.execute(f.run("run_2"), signal()), /busy/);
  await rejected;
  assert.throws(() => process.kill(pid, 0), { code: "ESRCH" });
});

test("a stale scope cannot cancel a new child or deliver its reply", async (t) => {
  const f = fixture(t);
  await f.carrier.execute(f.run(), signal());
  const ready = deferred();
  const pending = f.carrier.execute(f.run("run_2", (request) => ready.resolve(request)), signal());
  const request = await ready.promise;
  f.carrier.cancel(scope("run_1"));
  f.carrier.close({ window: "foreign", epoch: "foreign" });
  await assert.rejects(f.carrier.reply({ ...f.reply(request), session: "run_1" }), /foreign/);
  await f.carrier.reply(f.reply(request));
  assert.equal(await pending, expected);
});

test("a wrong current serial retires the worker and rejects that execution", async (t) => {
  const ready = deferred();
  const f = fixture(t);
  const pending = f.carrier.execute(f.run("run_1", (request) => ready.resolve(request)), signal());
  const rejected = assert.rejects(pending, /foreign/);
  const request = await ready.promise;
  await assert.rejects(f.carrier.reply({ ...f.reply(request), request_id: 2 }), /foreign/);
  await rejected;
});

test("request callback exceptions fail execution, while diagnostic exceptions do not", async (t) => {
  const f = fixture(t);
  await assert.rejects(f.carrier.execute(f.run("run_1", () => { throw new Error("request callback failed"); }), signal()),
    /request callback failed/);
  const g = fixture(t, "report_held", { onStderr() { throw new Error("display-only"); } });
  assert.equal(await g.carrier.execute(g.run(), signal()), expected);
});

test("close cancels pending description and permanently retires the carrier", async (t) => {
  const f = fixture(t, "description_hang");
  const pending = f.carrier.describe(signal());
  const rejected = assert.rejects(pending, /closed/);
  f.carrier.close(scope()); f.carrier.close(scope());
  await rejected;
  await assert.rejects(f.carrier.describe(signal()), /closed/);
  await assert.rejects(f.carrier.execute(f.run(), signal()), /closed/);
});

test("input admission and already-aborted signals never start an application request", async (t) => {
  const f = fixture(t);
  const controller = new AbortController(); controller.abort();
  await assert.rejects(f.carrier.describe(controller.signal), /cancelled/);
  await assert.rejects(f.carrier.execute(f.run(), controller.signal), /cancelled/);
  for (const changes of [{ source: "x".repeat(16 * 1024 + 1) }, { arguments: { text: "x".repeat(4097) } },
    { scope: { ...scope(), session: "bad scope" } }, { arguments: { text: "x", extra: "y" } }]) {
    await assert.rejects(f.carrier.execute({ ...f.run(), ...changes }, signal()));
  }
  assert.equal(f.received.length, 0);
  assert.equal(await f.carrier.execute(f.run(), signal()), expected);
  await assert.rejects(f.carrier.execute({ ...f.run("run_2"), scope: { ...scope("run_2"), epoch: "another" } }, signal()), /endpoint/);
});

test("missing executable fails cleanly without leaving the carrier busy", async (t) => {
  const f = fixture(t, "normal", { executable: "/private/tmp/leselang-worker-does-not-exist" });
  await assert.rejects(f.carrier.describe(signal()), /spawn_failed/);
  await assert.rejects(f.carrier.execute(f.run(), signal()), /spawn_failed/);
});

test("public options reject invalid paths, arguments and deadlines before spawning", () => {
  const options = { executable: process.execPath, catalog: (raw) => raw };
  for (const changed of [null, { executable: "relative-worker" }, { executable: 123 },
    { executable: `${process.execPath}\0` }, { executable: `/${"x".repeat(4096)}` },
    { catalog: null }, { args: "--serve" }, { args: [null] }, { args: ["bad\0argument"] },
    { args: Array(17).fill("argument") }, { onStderr: true }]) {
    assert.throws(() => createNodeAgentTransport(changed === null ? null : { ...options, ...changed }),
      /node_worker_invalid_options/);
  }
  for (const timeoutMs of [0, 19, 60_001, NaN, Infinity, 1.5, "1000"]) {
    assert.throws(() => createNodeAgentTransport({ ...options, timeoutMs }), /node_worker_invalid_timeout/);
  }
});

test("the carrier captures fixed launch arguments when constructed", async (t) => {
  const args = [worker, "normal"];
  const f = fixture(t, "normal", { args });
  args[1] = "report_nonzero";
  assert.equal(await f.carrier.execute(f.run(), signal()), expected);
});
