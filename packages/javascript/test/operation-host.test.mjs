import assert from "node:assert/strict";
import test from "node:test";
import { createNodeOperationHost } from "../dist/node/index.js";
import { createOperationRegistry } from "../dist/operations/index.js";

// Deliberately misbehaving peer for the parent-side checks. The independent
// Consumer runs these operations against the real Rust compiler and VM.
const peer = `
import { createInterface } from "node:readline";
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
const start = JSON.parse((await lines.next()).value);
const frame = (kind, fields) => process.stdout.write(JSON.stringify({kind, agent_worker_version:4, ...fields}) + "\\n");
const request = (serial) => frame("request", {challenge:"a".repeat(64), request:{
  ...start.scope, transport_version:1, request_id:serial, budget_ms:5000, operation:"test.touch", arguments:{}
}});
if (start.source.startsWith("failure")) {
  request(1);
  const {reply} = JSON.parse((await lines.next()).value);
  if (reply.status !== "uncertain") process.exit(5);
  if (start.source === "failure-next") request(2);
}
if (start.profile === "javascript.operations") process.exit(6); // Older v4 rejects every explicit profile field.
const profile = start.profile ?? "javascript.operations";
const report = {schema_version:1,kind:"operation_task_run",
  profile:start.source === "swap_profile" ? (profile === "lese.operations" ? "javascript.operations" : "lese.operations") : profile,
  effects:start.source === "extra_effect" ? 1 : 0, fuel_remaining:9999,
  result:{type:"integer",value:start.source === "number" ? 4 : "4"}};
frame("report", {report_json:JSON.stringify(report)});
process.stdin.destroy();
`;
function fixture(t, verifier, profile) {
  let calls = 0, checks = 0;
  const host = createNodeOperationHost({ executable: process.execPath, profile,
    args: ["--input-type=module", "-e", peer, "--"], registry: createOperationRegistry([
      { name: "test.touch", parameters: {}, result: "boolean", capability: "test.write",
        handle(_, context) { context.commit(() => { calls++; }); throw new Error("after commit"); } },
    ]), grants: () => ["test.write"], verify(evidence) { checks++; return verifier ? verifier(evidence) : true; } });
  t.after(() => host.close());
  const signal = new AbortController().signal;
  const plan = (source) => ({ source, summary: source, arguments: {} });
  return { host, calls: () => calls, checks: () => checks, signal, plan };
}

test("discovery, execution and verification pin the profile selected by the application", async (t) => {
  for (const selected of [undefined, "javascript.operations", "lese.operations"]) {
    const expected = selected ?? "javascript.operations";
    const f = fixture(t, ({ report }) => report.profile === expected, selected);
    assert.equal((await f.host.describe(f.signal)).profile, expected);
    await assert.rejects(f.host.execute(f.plan("swap_profile"), f.signal), /operation_invalid_report/);
    assert.equal(f.checks(), 0);
    const plan = f.plan("ok");
    const raw = await f.host.execute(plan, f.signal);
    assert.equal(JSON.parse(raw).profile, expected);
    assert.equal(await f.host.verify(raw, plan, f.signal), true);
  }
  for (const profile of [null, "unknown.operations", 4]) {
    assert.throws(() => fixture(t, undefined, profile), /operation_invalid_profile/);
  }
});

test("a worker cannot continue or complete after a failed operation", async (t) => {
  const f = fixture(t);
  for (const source of ["failure-report", "failure-next"]) {
    await assert.rejects(f.host.execute(f.plan(source), f.signal), /operation_after_failure/);
  }
  assert.equal(f.calls(), 2, "only each run's first side effect happened");
  assert.equal(f.checks(), 0);
});

test("the parent rejects invented effect counts and ill-typed result reports", async (t) => {
  const f = fixture(t);
  for (const source of ["extra_effect", "number"]) {
    await assert.rejects(f.host.execute(f.plan(source), f.signal), /invalid_(report|operation_integer)/);
  }
  assert.equal(f.checks(), 0);
});

test("operation verification consumes only the exact completed report and plan", async (t) => {
  const f = fixture(t);
  const plan = f.plan("ok");
  for (const mutation of ["report", "plan", "none"]) {
    const raw = await f.host.execute(plan, f.signal);
    assert.equal(await f.host.verify(raw, plan, new AbortController().signal), false);
    const checked = await f.host.verify(mutation === "report" ? raw + " " : raw,
      mutation === "plan" ? f.plan("different") : plan, f.signal);
    assert.equal(checked, mutation === "none");
    assert.equal(await f.host.verify(raw, plan, f.signal), false);
  }
  assert.equal(f.checks(), 1);
});

test("closing a host fences a verifier awaiting application evidence", async (t) => {
  const entered = Promise.withResolvers(), release = Promise.withResolvers();
  const f = fixture(t, async ({ signal }) => {
    entered.resolve(signal);
    await release.promise;
    return true;
  });
  const plan = f.plan("ok");
  const raw = await f.host.execute(plan, f.signal);
  const pending = f.host.verify(raw, plan, f.signal);
  const signal = await entered.promise;
  try {
    await assert.rejects(f.host.execute(plan, f.signal), /operation_host_busy/);
    f.host.close();
    assert.equal(signal.aborted, true);
  } finally { release.resolve(); }
  assert.equal(await pending, false);
});

test("v4 refuses rounded numeric tokens before dispatch and before admitting a report", async (t) => {
  const peer = `
    import { createInterface } from "node:readline";
    const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
    const { scope, arguments: { field, token } } = JSON.parse((await lines.next()).value);
    const output = (line) => process.stdout.write(line + "\\n");
    const substitute = (record) => JSON.stringify(record).replace('"NUMERIC_TOKEN"', token);
    let effects = 0;
    if (field === "request_id") {
      output(substitute({ agent_worker_version: 4, kind: "request", challenge: "a".repeat(64),
        request: { ...scope, transport_version: 1, request_id: "NUMERIC_TOKEN", budget_ms: 5000,
          operation: "test.touch", arguments: {} } }));
      if ((await lines.next()).done) process.exit(1);
      effects++;
    }
    const report = { schema_version: 1, kind: "operation_task_run", profile: "javascript.operations",
      result: { type: "none", value: null }, effects, fuel_remaining: 9999 };
    if (field !== "request_id") report[field] = "NUMERIC_TOKEN";
    output(JSON.stringify({ agent_worker_version: 4, kind: "report", report_json: substitute(report) }));
    process.stdin.destroy();
  `;
  let calls = 0;
  const host = createNodeOperationHost({ executable: process.execPath,
    args: ["--input-type=module", "-e", peer, "--"], registry: createOperationRegistry([
      { name: "test.touch", parameters: {}, result: "none", capability: "test.write",
        handle() { calls++; return null; } },
    ]), grants: () => ["test.write"], verify: () => true });
  t.after(() => host.close());
  for (const [field, token] of [
    ["request_id", "1.0000000000000000001"],
    ["effects", "1e-999"],
    ["fuel_remaining", "9999.0000000000001"],
    ["schema_version", "1e0"],
  ]) {
    const plan = { source: "test numeric metadata", summary: field, arguments: { field, token }, inputMode: "named_strings" };
    await assert.rejects(host.execute(plan, new AbortController().signal), /node_worker_invalid_json/);
  }
  assert.equal(calls, 0);
});
