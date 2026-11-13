import assert from "node:assert/strict";
import test from "node:test";
import { AgentSession } from "@leselang/bridge";

const source = "fn main(text: string) = true";
const plan = (text = "literal") => ({ summary: "Save the supplied text", source, arguments: { text } });
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function until(predicate) {
  for (let index = 0; index < 20 && !predicate(); index += 1) await Promise.resolve();
  assert.ok(predicate(), "expected stage reached");
}
function fixture(overrides = {}) {
  const calls = { describe: 0, plan: 0, execute: 0, verify: 0 };
  const events = [];
  const session = new AgentSession({
    provider: {
      async plan(context, signal) {
        calls.plan += 1;
        return overrides.plan ? overrides.plan(context, signal) : plan(context.message);
      },
    },
    host: {
      async describe(signal) {
        calls.describe += 1;
        return overrides.describe ? overrides.describe(signal) : { profile: "reference", operations: ["save"] };
      },
      async execute(value, signal) {
        calls.execute += 1;
        return overrides.execute ? overrides.execute(value, signal) : { saved: value.arguments.text, verified: true };
      },
      verify(report, value, signal) {
        calls.verify += 1;
        return overrides.verify ? overrides.verify(report, value, signal) : report.saved === value.arguments.text;
      },
    },
    onEvent(state) {
      events.push(state);
      overrides.event?.(state, session);
    },
  });
  return { session, calls, events };
}

test("explicit stages verify literal data and later submissions use fresh task identities", async () => {
  const f = fixture();
  assert.equal(f.session.state.phase, "idle");
  assert.equal(f.session.busy, false);
  for (const [index, message] of ['保存 "✓" \\\n', "--execute"].entries()) {
    const result = await f.session.submit(message);
    assert.equal(result.taskId, index + 1);
    assert.equal(result.status, "verified");
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(result.plan.source, source);
    assert.equal(result.plan.arguments.text, message);
    assert.equal(result.report.saved, message);
    assert.ok(Object.isFrozen(result));
    assert.equal(f.session.busy, false);
  }
  assert.deepEqual(f.events.map((event) => event.phase), [
    "describing", "planning", "executing", "verifying", "verified",
    "describing", "planning", "executing", "verifying", "verified",
  ]);
  assert.deepEqual(f.calls, { describe: 2, plan: 2, execute: 2, verify: 2 });
  assert.equal(f.session.cancel(), false);
});

test("catalog, admitted plan, report and observer state own immutable data", async () => {
  const catalog = { profile: "reference", operations: [{ name: "save" }] };
  const proposed = plan();
  const report = { saved: "literal", nested: [true] };
  const f = fixture({
    describe: async () => catalog,
    async plan(context) {
      catalog.operations[0].name = "mutated-original";
      assert.equal(context.catalog.operations[0].name, "save");
      assert.ok(Object.isFrozen(context));
      assert.ok(Object.isFrozen(context.catalog.operations[0]));
      return proposed;
    },
    event(state) {
      assert.ok(Object.isFrozen(state));
      if (state.phase === "executing") proposed.arguments.text = "modified after admission";
    },
    async execute(admitted) {
      assert.equal(admitted.arguments.text, "literal");
      assert.ok(Object.isFrozen(admitted));
      assert.ok(Object.isFrozen(admitted.arguments));
      return report;
    },
    verify(observed) {
      report.saved = "mutated-original";
      report.nested[0] = false;
      assert.equal(observed.saved, "literal");
      assert.deepEqual([...observed.nested], [true]);
      assert.ok(Object.isFrozen(observed.nested));
      return true;
    },
  });
  assert.equal((await f.session.submit("request")).status, "verified");
});

test("only the host's strict true verification establishes success", async () => {
  for (const verified of [false, "true", 1, null, undefined]) {
    const f = fixture({ verify: () => verified });
    const result = await f.session.submit("task");
    assert.equal(result.status, "failed");
    assert.equal(result.error, "application_verification_failed");
    assert.equal(result.possiblePartialEffects, true);
    assert.equal(result.report.verified, true, "a report's claim alone has no authority");
    assert.equal(f.calls.execute, 1);
    assert.equal(f.calls.plan, 1);
  }
});

test("invalid messages are rejected before discovery or task allocation", async () => {
  const f = fixture();
  for (const message of ["", " \n", "x".repeat(8193), "界".repeat(2731), "\ud800", "\udc00", 42, null]) {
    await assert.rejects(f.session.submit(message), /invalid_message/);
    assert.equal(f.session.busy, false);
  }
  assert.equal(f.calls.describe, 0);
  assert.equal((await f.session.submit("valid")).taskId, 1);
});

test("wrong plan fields, coercions, invalid UTF-16 and byte limits never reach execution", async () => {
  let getterCalled = false;
  const accessor = { ...plan(), get source() { getterCalled = true; return source; } };
  const variants = [
    null, [], { ...plan(), extra: true }, { summary: "x", source },
    { ...plan(), arguments: { text: "x", extra: true } },
    { ...plan(), arguments: { text: 42 } }, { ...plan(), summary: "" },
    { ...plan(), source: " \n" }, { ...plan(), source: "x".repeat(16385) },
    { ...plan(), source: "界".repeat(5462) }, { ...plan(), summary: "x".repeat(1025) },
    { ...plan(), arguments: { text: "界".repeat(1366) } },
    { ...plan(), arguments: { text: "\ud800" } }, { ...plan(), summary: "\udc00" },
    { ...plan(), task: 42 }, { ...plan(), task: "bad.task" }, { ...plan(), task: "_lese_hidden" },
    { ...plan(), arguments: { "bad.name": "x" } }, { ...plan(), arguments: { body: "x" } },
    Object.assign(Object.create({ inherited: true }), plan()), accessor,
  ];
  for (const value of variants) {
    const f = fixture({ plan: async () => value });
    const result = await f.session.submit("task");
    assert.equal(result.status, "failed");
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(f.calls.execute, 0);
    assert.equal(f.calls.verify, 0);
  }
  assert.equal(getterCalled, false);
});

test("non-JSON, cyclic, deep or oversized catalogs stop before planning", async () => {
  const cycle = {}; cycle.self = cycle;
  let deep = {};
  for (let index = 0; index < 34; index += 1) deep = { next: deep };
  const hole = []; hole.length = 1;
  for (const catalog of [undefined, NaN, Infinity, 1n, { callback() {} }, cycle, deep, hole,
    { text: "x".repeat(256 * 1024) }, { text: "\0".repeat(50_000) }, Array(8193).fill(true)]) {
    const f = fixture({ describe: async () => catalog });
    const result = await f.session.submit("task");
    assert.equal(result.status, "failed");
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(f.calls.plan, 0);
    assert.equal(f.calls.execute, 0);
  }
});

test("declared tasks retain zero or one named string input without changing legacy plans", async () => {
  for (const arguments_ of [{}, { message: 'quote " slash \\ newline\n✓' }]) {
    const original = { task: "publish_note", summary: "Publish", source: "fn main() = true", arguments: arguments_ };
    const f = fixture({
      plan: async () => original,
      execute: async (value) => {
        assert.equal(value.task, "publish_note");
        assert.deepEqual({ ...value.arguments }, arguments_);
        assert.ok(Object.isFrozen(value.arguments));
        return true;
      },
      verify: () => true,
    });
    assert.equal((await f.session.submit("task publish_note")).status, "verified");
  }
  assert.equal((await fixture().session.submit("legacy")).plan.task, undefined);
});

test("raw catalogs preserve integer tokens and use the catalog wire limit without string re-encoding", async () => {
  const raw = '{"bound":9223372036854775807,"padding":"' + "\\n".repeat(85_000) + '"}';
  const f = fixture({
    describe: async () => raw,
    plan: async (context) => { assert.equal(context.catalog, raw); return plan(); },
  });
  assert.equal((await f.session.submit("literal")).status, "verified");
  const limit = 256 * 1024;
  for (const extra of [0, 1]) {
    const value = " ".repeat(limit + extra);
    const f = fixture({ describe: async () => value });
    assert.equal((await f.session.submit("task")).status, extra ? "failed" : "verified");
    assert.equal(f.calls.plan, extra ? 0 : 1);
  }
  const hugeReport = fixture({ execute: async () => "x".repeat(128 * 1024 + 1) });
  assert.equal((await hugeReport.session.submit("task")).status, "failed");
  assert.equal(hugeReport.calls.verify, 0);
});

test("invalid execution reports cannot reach application verification", async () => {
  for (const report of [undefined, { callback() {} }, { text: "x".repeat(128 * 1024) }, "\ud800"]) {
    const f = fixture({ execute: async () => report });
    const result = await f.session.submit("task");
    assert.equal(result.status, "failed");
    assert.equal(result.possiblePartialEffects, true);
    assert.equal(f.calls.verify, 0);
    assert.equal(result.report, undefined);
  }
});

test("cancellation during discovery waits for settlement and never starts a provider", async () => {
  const wait = deferred();
  let signal;
  const f = fixture({ describe(value) { signal = value; return wait.promise; } });
  const pending = f.session.submit("task");
  assert.equal(f.session.cancel(), true);
  assert.equal(f.session.cancel(), false);
  assert.equal(signal.aborted, true);
  assert.equal(f.session.state.phase, "cancelling");
  assert.equal(f.session.busy, true);
  await assert.rejects(f.session.submit("overlap"), /agent_task_pending/);
  wait.resolve({ profile: "late" });
  const result = await pending;
  assert.equal(result.status, "cancelled");
  assert.equal(result.possiblePartialEffects, false);
  assert.equal(f.calls.plan, 0);
  assert.equal(f.calls.execute, 0);
  assert.equal(f.session.busy, false);
});

test("a cancelled uncooperative provider cannot execute its late valid proposal", async () => {
  const wait = deferred();
  const f = fixture({ plan: () => wait.promise });
  const pending = f.session.submit("task");
  await until(() => f.calls.plan === 1);
  assert.equal(f.session.cancel(), true);
  await assert.rejects(f.session.submit("new task"), /agent_task_pending/);
  wait.resolve(plan());
  const result = await pending;
  assert.equal(result.status, "cancelled");
  assert.equal(result.possiblePartialEffects, false);
  assert.equal(f.calls.execute, 0);
  assert.equal(result.plan, undefined);
  assert.equal((await f.session.submit("next task")).status, "verified");
  assert.equal(f.session.state.taskId, 2);
});

test("cancellation during execution fences late replies and retains partial-effect uncertainty", async () => {
  const wait = deferred();
  let sideEffects = 0;
  let signal;
  const f = fixture({ execute(value, active) {
    signal = active;
    sideEffects += 1;
    return wait.promise;
  } });
  const pending = f.session.submit("literal");
  await until(() => f.calls.execute === 1);
  assert.equal(f.session.state.possiblePartialEffects, true);
  f.session.cancel();
  assert.equal(signal.aborted, true);
  assert.equal(f.session.busy, true);
  await assert.rejects(f.session.submit("overlap"), /agent_task_pending/);
  wait.resolve({ saved: "literal", verified: true });
  const result = await pending;
  assert.equal(result.status, "cancelled");
  assert.equal(result.possiblePartialEffects, true);
  assert.equal(sideEffects, 1);
  assert.equal(f.calls.verify, 0);
  assert.equal(result.report, undefined);
  assert.equal(f.session.state.phase, "cancelled");
});

test("cancelling asynchronous verification prevents a late true from becoming success", async () => {
  const wait = deferred();
  const f = fixture({ verify: () => wait.promise });
  const pending = f.session.submit("task");
  await until(() => f.calls.verify === 1);
  f.session.cancel();
  wait.resolve(true);
  const result = await pending;
  assert.equal(result.status, "cancelled");
  assert.equal(result.possiblePartialEffects, true);
  assert.ok(!f.events.some((state) => state.phase === "verified"));
});

test("provider and host errors retain the correct execution boundary without retry", async () => {
  for (const stage of ["describe", "plan", "execute", "verify"]) {
    const f = fixture({ [stage]: () => { throw new Error(`${stage}_failed`); } });
    const result = await f.session.submit("task");
    assert.equal(result.status, "failed");
    assert.equal(result.error, `${stage}_failed`);
    assert.equal(result.possiblePartialEffects, ["execute", "verify"].includes(stage));
    assert.ok(Object.values(f.calls).every((count) => count <= 1));
    assert.equal(f.session.busy, false);
  }
  const f = fixture({ plan: () => { throw new Error("x".repeat(2000)); } });
  assert.equal((await f.session.submit("task")).error, "agent_task_failed");
});

test("observers cannot bypass fences, and observer failures cannot grant success", async () => {
  for (const phase of ["describing", "planning", "executing"]) {
    const f = fixture({ event(state, session) { if (state.phase === phase) session.cancel(); } });
    const result = await f.session.submit("task");
    assert.equal(result.status, "cancelled");
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(f.calls.execute, 0);
  }
  const f = fixture({ event() { throw new Error("display failed"); }, verify: () => false });
  assert.equal((await f.session.submit("task")).status, "failed");
});

test("a terminal observer can submit a new task without old cleanup clearing it", async () => {
  const wait = deferred();
  let second;
  const f = fixture({
    async execute(value) { return value.arguments.text === "second" ? wait.promise : { saved: value.arguments.text }; },
    event(state, session) { if (state.phase === "verified" && state.taskId === 1) second = session.submit("second"); },
  });
  assert.equal((await f.session.submit("first")).status, "verified");
  await until(() => f.calls.execute === 2);
  assert.equal(f.session.busy, true);
  assert.equal(f.session.state.taskId, 2);
  wait.resolve({ saved: "second" });
  assert.equal((await second).status, "verified");
  assert.equal(f.session.busy, false);
});
