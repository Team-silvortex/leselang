import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { AgentWorkflowSession, prepareSemanticCatalog } from "@leselang/bridge";

const raw = await readFile(new URL("../../../fixtures/host-semantics/tauri-agent-semantics-v1.json", import.meta.url), "utf8");
const task = (id, args = {}, catalog = raw) => prepareSemanticCatalog(catalog).prepare(id, args).plan;
const workflow = () => ({ steps: [task("inspect_controls"), task("save_message", { message: 'literal "✓" \\ \ntext' }), task("check_controls")] });
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function until(predicate) {
  for (let index = 0; index < 100 && !predicate(); index++) await Promise.resolve();
  assert.ok(predicate(), "expected workflow stage reached");
}
function fixture(overrides = {}) {
  const calls = { describe: 0, plan: 0, execute: 0, verify: 0 };
  const trace = [], events = [], reports = [];
  const session = new AgentWorkflowSession({
    provider: { async plan(context, signal) {
      calls.plan++; trace.push("plan");
      return overrides.plan ? overrides.plan(context, signal) : workflow();
    } },
    host: {
      async describe(signal) {
        calls.describe++; trace.push("describe");
        return overrides.describe ? overrides.describe(signal, calls.describe) : raw;
      },
      async execute(plan, signal) {
        calls.execute++; trace.push(`execute:${plan.task}`);
        const result = overrides.execute ? await overrides.execute(plan, signal, calls.execute) : { task: plan.task, effects: calls.execute };
        reports.push(result); return result;
      },
      async verify(report, plan, signal) {
        calls.verify++; trace.push(`verify:${plan.task}`);
        return overrides.verify ? overrides.verify(report, plan, signal, calls.verify) : report.task === plan.task;
      },
      ...(overrides.verifyDetailed ? { async verifyDetailed(report, plan, signal) {
        calls.detailed = (calls.detailed ?? 0) + 1;
        return overrides.verifyDetailed(report, plan, signal, calls.detailed);
      } } : {}),
    },
    onEvent(state) { events.push(state); overrides.event?.(state, session); },
  });
  return { session, calls, trace, events, reports };
}

test("one finite proposal runs three separately verified steps in order", async () => {
  const f = fixture();
  assert.deepEqual(f.session.state, { taskId: 0, phase: "idle", possiblePartialEffects: false, totalSteps: 0, completedSteps: 0 });
  const result = await f.session.submit("inspect, save, then check");
  assert.equal(result.status, "verified");
  assert.equal(result.taskId, 1);
  assert.equal(result.possiblePartialEffects, false);
  assert.equal(result.steps.length, 3);
  assert.deepEqual(result.steps.map((step) => step.taskId), [1, 2, 3]);
  assert.deepEqual(result.steps.map((step) => step.status), ["verified", "verified", "verified"]);
  assert.deepEqual(f.calls, { describe: 4, plan: 1, execute: 3, verify: 3 });
  assert.deepEqual(f.trace, ["describe", "plan", "describe", "execute:inspect_controls", "verify:inspect_controls", "describe", "execute:save_message", "verify:save_message", "describe", "execute:check_controls", "verify:check_controls"]);
  assert.equal(f.session.state.completedSteps, 3);
  assert.equal(f.session.state.totalSteps, 3);
  assert.equal(f.events.filter((event) => event.phase === "verified").length, 1);
  assert.equal(f.session.busy, false);
  assert.equal(f.session.cancel(), false);
  assert.ok(Object.isFrozen(result) && Object.isFrozen(result.steps) && Object.isFrozen(result.plan.steps));
  assert.equal((await f.session.submit("again explicitly")).taskId, 2);
});

test("whole-plan admission rejects an invalid final step before any application execution", async () => {
  const valid = task("check_controls");
  const invalid = [
    { ...valid, task: "not_declared" },
    { ...valid, arguments: { extra: "input" } },
    { ...valid, source: "fn main() = true" },
    { ...valid, verified: true },
    { ...valid, task: undefined },
    { ...valid, summary: "x".repeat(1025) },
    { ...task("save_message", { message: "x" }), arguments: { message: "\0" } },
  ];
  for (const last of invalid) {
    const f = fixture({ plan: async () => ({ steps: [task("save_message", { message: "must not save" }), last] }) });
    const result = await f.session.submit("invalid second step");
    assert.equal(result.status, "failed");
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(result.steps.length, 0);
    assert.equal(f.calls.execute, 0);
    assert.equal(f.calls.describe, 1);
  }
});

test("finite plan bounds and property ownership reject hooks, holes, cycles and extra fields", async () => {
  let hooks = 0;
  const get = () => { hooks++; throw new Error("should not run"); };
  const values = [null, {}, { steps: [] }, { steps: Array(9).fill(task("check_controls")) },
    { steps: [task("check_controls")], extra: true }, { get steps() { return get(); } },
    { steps: new Array(1) }, { steps: Object.defineProperty([], "0", { get, enumerable: true }) },
    { steps: [{ ...task("check_controls"), arguments: { get input() { return get(); } } }] },
    { steps: [{ ...task("check_controls"), toJSON: get }] }];
  const cyclic = {}; cyclic.steps = [cyclic]; values.push(cyclic);
  const symbol = { steps: [task("check_controls")] }; symbol[Symbol("hidden")] = true; values.push(symbol);
  for (const value of values) {
    const f = fixture({ plan: async () => value });
    assert.equal((await f.session.submit("bad proposal")).status, "failed");
    assert.equal(f.calls.execute, 0);
  }
  assert.equal(hooks, 0);
  const max = fixture({ plan: async () => ({ steps: Array(8).fill(task("check_controls")) }) });
  assert.equal((await max.session.submit("eight checks")).status, "verified");
  assert.equal(max.calls.execute, 8);
});

test("the admitted proposal is immutable and summaries come from host declarations", async () => {
  const proposed = { steps: workflow().steps.map((step) => ({ ...step, arguments: { ...step.arguments }, summary: "provider claim" })) };
  const f = fixture({ plan: async () => proposed, event(state) {
    assert.ok(Object.isFrozen(state));
    if (state.totalSteps) {
      proposed.steps[1].arguments.message = "changed";
      proposed.steps[2].source = "bad";
    }
  } });
  const result = await f.session.submit("owned proposal");
  assert.equal(result.status, "verified");
  assert.equal(result.plan.steps[1].arguments.message, 'literal "✓" \\ \ntext');
  assert.notEqual(result.plan.steps[1].summary, "provider claim");
  assert.equal(Object.isFrozen(result.plan.steps[1].arguments), true);
  assert.throws(() => { result.steps[0].report.task = "modified"; });
});

test("a failed second verification keeps evidence and delivered work without starting the third", async () => {
  let writes = 0;
  const f = fixture({
    execute: async (plan) => { if (plan.task === "save_message") writes++; return { task: plan.task, writes }; },
    verify: async (_report, _plan, _signal, count) => count !== 2,
  });
  const result = await f.session.submit("inspect, save, check");
  assert.equal(result.status, "failed");
  assert.equal(result.error, "application_verification_failed");
  assert.equal(result.possiblePartialEffects, true);
  assert.equal(result.stoppedStep, 1);
  assert.equal(result.steps.length, 2);
  assert.equal(result.steps[0].status, "verified");
  assert.equal(result.steps[1].report.writes, 1);
  assert.equal(writes, 1);
  assert.equal(f.calls.execute, 2);
  assert.equal(f.session.state.completedSteps, 1);
  assert.equal(Object.hasOwn(result, "verificationFailure"), false);
});

test("a detailed child failure reaches workflow outcome and state without repeating verification", async () => {
  const failure = { code: "after_observation_mismatch" };
  const f = fixture({ verifyDetailed: async (_report, _plan, _signal, count) =>
    count === 2 ? { verified: false, diagnostic: failure } : { verified: true } });
  const result = await f.session.submit("inspect, save, check");
  failure.code = "changed_later";
  assert.equal(result.status, "failed");
  assert.equal(result.stoppedStep, 1);
  assert.equal(result.steps.length, 2);
  assert.equal(result.possiblePartialEffects, true);
  assert.equal(result.error, "application_verification_failed");
  assert.deepEqual(result.verificationFailure, { code: "after_observation_mismatch" });
  assert.equal(result.verificationFailure, result.steps[1].verificationFailure);
  assert.equal(result.verificationFailure, f.session.state.verificationFailure);
  assert.ok(Object.isFrozen(result.verificationFailure));
  assert.equal(f.calls.verify, 0);
  assert.equal(f.calls.detailed, 2);
  assert.equal(f.calls.execute, 2);
  assert.equal(f.session.state.completedSteps, 1);
});

test("workflow cancellation during detailed verification omits late failure diagnostics", async () => {
  const pending = deferred();
  const f = fixture({ verifyDetailed: () => pending.promise });
  const execution = f.session.submit("cancel detailed verification");
  await until(() => f.calls.detailed === 1);
  f.session.cancel();
  pending.resolve({ verified: false, diagnostic: { code: "before_observation_mismatch" } });
  const result = await execution;
  assert.equal(result.status, "cancelled");
  assert.equal(result.steps.length, 1);
  assert.equal(result.steps[0].status, "cancelled");
  assert.equal(result.possiblePartialEffects, true);
  assert.equal(Object.hasOwn(result, "verificationFailure"), false);
  assert.equal(Object.hasOwn(result.steps[0], "verificationFailure"), false);
  assert.equal(Object.hasOwn(f.session.state, "verificationFailure"), false);
  assert.equal(f.calls.verify, 0);
  assert.equal(f.calls.execute, 1);
});

test("workflow detailed wrapper rejects synchronous thenables without invoking their getters", async () => {
  let getters = 0;
  // Keep the application callback synchronous; an async adapter owns its own assimilation.
  const host = {
    async describe() { return raw; }, async execute() { return {}; }, verify() { throw new Error("legacy verifier must not run"); },
    verifyDetailed() { return { get then() { getters++; return (resolve) => resolve({ verified: true }); } }; },
  };
  const session = new AgentWorkflowSession({ host, provider: { async plan() { return workflow(); } } });
  const result = await session.submit("reject malformed detail");
  assert.equal(result.status, "failed");
  assert.deepEqual(result.verificationFailure, { code: "invalid_verification_result" });
  assert.equal(result.steps.length, 1);
  assert.equal(getters, 0);
});

test("later execute exceptions and report failures retain earlier outcomes and stop once", async () => {
  for (const fault of ["throw", "oversized"]) {
    const f = fixture({ execute: async (plan, _signal, count) => {
      if (count === 2) {
        if (fault === "throw") throw new Error("delivered then failed");
        return "x".repeat(128 * 1024 + 1);
      }
      return { task: plan.task };
    } });
    const result = await f.session.submit("failed sequence");
    assert.equal(result.status, "failed");
    assert.equal(result.possiblePartialEffects, true);
    assert.equal(result.steps[0].status, "verified");
    assert.equal(result.steps[1].status, "failed");
    assert.equal(f.calls.execute, 2);
    assert.equal(f.calls.verify, 1);
  }
});

test("catalog snapshots are pinned before each step including verification-only metadata", async () => {
  const changed = raw.replace("9223372036854775807", "9223372036854775806");
  assert.notEqual(changed, raw);
  assert.equal(task("inspect_controls", {}, changed).source, task("inspect_controls").source);
  for (const changeAt of [2, 3]) {
    const f = fixture({ describe: async (_signal, count) => count >= changeAt ? changed : raw });
    const result = await f.session.submit("pin contract");
    assert.equal(result.status, "failed");
    assert.equal(result.error, "workflow_catalog_changed");
    assert.equal(result.possiblePartialEffects, changeAt === 3);
    assert.equal(f.calls.execute, changeAt - 2);
    assert.equal(result.stoppedStep, changeAt - 2);
  }
  // Byte identity is deliberately conservative, including harmless whitespace.
  const f = fixture({ describe: async (_signal, count) => count === 1 ? raw : ` ${raw}` });
  assert.equal((await f.session.submit("changed bytes")).error, "workflow_catalog_changed");
  assert.equal(f.calls.execute, 0);
});

test("unknown profiles and renamed inputs do not require a workflow strategy branch", async () => {
  const changed = raw.replaceAll("tauri.typescript", "custom.notes")
    .replaceAll("gui.", "notes.").replaceAll('"save_message"', '"publish_note"')
    .replaceAll('"message"', '"content"');
  const f = fixture({ describe: async () => changed, plan: async () => ({
    steps: [task("inspect_controls", {}, changed), task("publish_note", { content: "new app" }, changed)],
  }) });
  const result = await f.session.submit("inspect and publish");
  assert.equal(result.status, "verified");
  assert.equal(result.plan.steps[1].arguments.content, "new app");
  assert.match(result.plan.steps[1].source, /notes\.set_text/u);
});

test("cancellation of initial describe or provider waits for real settlement without execution", async () => {
  for (const stage of ["describe", "plan"]) {
    const pending = deferred(); let forwarded;
    const f = fixture({ [stage]: async (...args) => {
      forwarded = stage === "describe" ? args[0] : args[1]; return pending.promise;
    } });
    const resultPromise = f.session.submit("waiting");
    await until(() => forwarded !== undefined);
    assert.equal(f.session.cancel(), true);
    assert.equal(forwarded.aborted, true);
    assert.equal(f.session.cancel(), false);
    assert.equal(f.session.busy, true);
    await assert.rejects(f.session.submit("overlap"), /agent_task_pending/u);
    pending.resolve(stage === "describe" ? raw : workflow());
    const result = await resultPromise;
    assert.equal(result.status, "cancelled");
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(f.calls.execute, 0);
    assert.equal(f.session.busy, false);
  }
});

test("cancellation while executing or verifying retains prior evidence and rejects late continuation", async () => {
  for (const stage of ["execute", "verify"]) {
    const pending = deferred(); let forwarded;
    const f = fixture({ [stage]: async (...args) => {
      const count = args.at(-1);
      if (count === 2) { forwarded = stage === "execute" ? args[1] : args[2]; return pending.promise; }
      return stage === "execute" ? { task: args[0].task } : true;
    } });
    const resultPromise = f.session.submit("waiting later");
    await until(() => forwarded !== undefined);
    f.session.cancel();
    assert.equal(forwarded.aborted, true);
    assert.equal(f.session.state.possiblePartialEffects, true);
    assert.equal(f.session.busy, true);
    await assert.rejects(f.session.submit("no overlap"), /agent_task_pending/u);
    pending.resolve(stage === "execute" ? { task: "save_message" } : true);
    const result = await resultPromise;
    assert.equal(result.status, "cancelled");
    assert.equal(result.possiblePartialEffects, true);
    assert.deepEqual(result.steps.map((step) => step.status), ["verified", "cancelled"]);
    assert.equal(f.calls.execute, 2);
    assert.equal(f.calls.verify, stage === "execute" ? 1 : 2);
    if (stage === "verify") assert.equal(result.steps[1].report.task, "save_message");
  }
});

test("event cancellation before dispatch or between verified steps prevents later application work", async () => {
  for (const target of ["first-dispatch", "between", "second-describe"]) {
    let cancelled = false;
    const f = fixture({ event(state, session) {
      if (cancelled) return;
      if ((target === "first-dispatch" && state.phase === "executing")
        || (target === "between" && state.completedSteps === 1)
        || (target === "second-describe" && state.stepIndex === 1 && state.phase === "describing")) {
        cancelled = true; session.cancel();
      }
    } });
    const result = await f.session.submit("cancel on boundary");
    assert.equal(result.status, "cancelled");
    assert.equal(f.calls.execute, target === "first-dispatch" ? 0 : 1);
    assert.equal(result.possiblePartialEffects, target !== "first-dispatch");
    assert.equal(result.stoppedStep, target === "first-dispatch" ? 0 : 1);
  }
});

test("bad catalog ownership and malformed user text fail without invoking provider or host work", async () => {
  let hooks = 0;
  for (const catalog of ["{}", JSON.parse(raw), { get semantics() { hooks++; return {}; } },
    { toJSON() { hooks++; return {}; } }, " ".repeat(256 * 1024 + 1)]) {
    const f = fixture({ describe: async () => catalog });
    const result = await f.session.submit("bad catalog");
    assert.equal(result.status, "failed");
    assert.equal(f.calls.plan, 0);
    assert.equal(f.calls.execute, 0);
  }
  assert.equal(hooks, 0);
  for (const message of ["", " ", "\ud800", "中".repeat(2731), 12]) {
    const f = fixture();
    await assert.rejects(f.session.submit(message));
    assert.equal(f.calls.describe, 0);
  }
});

test("owned object catalogs cannot be changed by provider and observer exceptions do not grant authority", async () => {
  const object = JSON.parse(raw.replaceAll("9223372036854775807", "9007199254740991"));
  const f = fixture({ describe: async () => object, plan: async (context) => {
    assert.ok(Object.isFrozen(context.catalog.semantics.tasks));
    assert.throws(() => { context.catalog.semantics.tasks[0].id = "changed"; });
    return { steps: [task("check_controls", {}, object)] };
  }, event() { throw new Error("display failed"); } });
  assert.equal((await f.session.submit("object catalog")).status, "verified");
});
