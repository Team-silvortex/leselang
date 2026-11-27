import assert from "node:assert/strict";
import test from "node:test";
import { AgentSession } from "@leselang/bridge";
import { createNodeOperationHost } from "@leselang/bridge/node";
import { createOperationRegistry } from "@leselang/bridge/operations";
import { createStockAgent, pipelineProvider, worker } from "./operations.mjs";

test("an agent discovers typed inventory operations and verifies two real worker Effects", async () => {
  const app = createStockAgent();
  const label = 'bin "15"\n备件';
  try {
    const outcome = await app.session.submit("stock.reserve |> stock.remaining\n" + label);
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(outcome.plan.arguments.input, label);
    assert.equal(outcome.plan.source.includes(label), false);
    assert.match(outcome.plan.source, /stock\.reserve\(item: input\)/u);
    assert.match(outcome.plan.source, /stock\.remaining\(ticket: previous\)/u);
    const report = JSON.parse(outcome.report);
    assert.equal(report.kind, "operation_task_run");
    assert.equal(report.profile, "javascript.operations");
    assert.equal(report.effects, 2);
    assert.ok(report.fuel_remaining > 0 && report.fuel_remaining < 10_000);
    assert.deepEqual(report.result, { type: "integer", value: "9007199254740998" });
    assert.deepEqual(app.snapshot(), { available: 9007199254740998n, reservations: [[9007199254740993n, label]] });
  } finally { app.close(); }
});

// A second application uses two different operation names, parameter names
// and result types. The planner is the same public AgentProvider instance.
const translate = "dictionary.translate |> archive.record\n";
function archive(t, { beforeCommit, reply, store = (items, translation) => items.push(translation), grants } = {}) {
  const entries = new Map([["green", "vert"], ["red", "rouge"]]);
  const recorded = [], calls = [];
  const registry = createOperationRegistry([
    { name: "dictionary.translate", parameters: { word: "string" }, result: "optional_string", capability: "dictionary.read",
      handle({ word }) { calls.push("translate"); return entries.get(word) ?? null; } },
    { name: "archive.record", parameters: { translation: "optional_string" }, result: "boolean", capability: "archive.write",
      async handle({ translation }, context) {
        calls.push("record");
        await beforeCommit?.(context);
        context.commit(() => store(recorded, translation));
        return reply ? reply(translation) : true;
      } },
  ]);
  const host = createNodeOperationHost({ executable: worker, registry,
    grants: grants ?? (() => ["dictionary.read", "archive.write"]),
    observe: () => ({ count: recorded.length, last: recorded.at(-1) ?? null }),
    verify: ({ plan, report, before, after }) => report.effects === 2 && report.result.type === "boolean" &&
      report.result.value === true && after.count === before.count + 1 &&
      after.last === (entries.get(plan.arguments.input) ?? null),
  });
  const session = new AgentSession({ host, provider: pipelineProvider });
  t.after(() => { session.cancel(); host.close(); });
  return { session, host, recorded, calls };
}

test("the same provider composes unrelated optional-string and boolean signatures, including absence", async (t) => {
  const app = archive(t);
  for (const input of ["green", "untranslated"]) {
    const outcome = await app.session.submit(translate + input);
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(JSON.parse(outcome.report).result, { type: "boolean", value: true });
    assert.match(outcome.plan.source, /dictionary\.translate\(word: input\)/u);
    assert.match(outcome.plan.source, /archive\.record\(translation: previous\)/u);
  }
  assert.deepEqual(app.recorded, ["vert", null]);
});

test("an unavailable grant rejects the complete program before its first callback", async (t) => {
  const grants = ["dictionary.read"];
  const app = archive(t, { grants: () => grants });
  const failed = await app.session.submit(translate + "red");
  assert.equal(failed.status, "failed");
  assert.equal(failed.report, undefined);
  assert.deepEqual(app.calls, []);
  grants.push("archive.write");
  const next = await app.session.submit(translate + "red");
  assert.equal(next.status, "verified", next.error);
  assert.deepEqual(app.recorded, ["rouge"]);
});

test("an incorrect handler result after commit fails the run and retains the application effect", async (t) => {
  const app = archive(t, { reply: () => "true" });
  const failed = await app.session.submit(translate + "green");
  assert.equal(failed.status, "failed");
  assert.equal(failed.possiblePartialEffects, true);
  assert.equal(failed.report, undefined);
  assert.deepEqual(app.calls, ["translate", "record"]);
  assert.deepEqual(app.recorded, ["vert"]);
});

test("valid VM completion can still fail independent application verification", async (t) => {
  const app = archive(t, { store: (items) => items.push("different translation") });
  const failed = await app.session.submit(translate + "green");
  assert.equal(failed.status, "failed");
  assert.equal(failed.error, "application_verification_failed");
  assert.equal(failed.possiblePartialEffects, true);
  assert.deepEqual(JSON.parse(failed.report).result, { type: "boolean", value: true });
  assert.deepEqual(app.recorded, ["different translation"]);
});

test("an earlier verified write cannot satisfy a repeated request", async (t) => {
  const app = archive(t, { store(items, translation) {
    if (!items.includes(translation)) items.push(translation);
  } });
  const first = await app.session.submit(translate + "red");
  assert.equal(first.status, "verified", first.error);
  const second = await app.session.submit(translate + "red");
  assert.equal(second.status, "failed");
  assert.equal(second.error, "application_verification_failed");
  assert.equal(second.report, first.report);
  assert.deepEqual(app.calls, ["translate", "record", "translate", "record"]);
  assert.deepEqual(app.recorded, ["rouge"]);
});

for (const stop of ["cancel", "revoke"]) {
  test(`a real worker Effect waiting inside the handler cannot commit after ${stop}`, { timeout: 10000 }, async (t) => {
    const entered = Promise.withResolvers(), release = Promise.withResolvers();
    let write = true, late;
    const app = archive(t, {
      grants: () => write ? ["dictionary.read", "archive.write"] : ["dictionary.read"],
      async beforeCommit(context) {
        if (late) return;
        late = context;
        entered.resolve();
        await release.promise;
      },
    });
    const pending = app.session.submit(translate + "red");
    try {
      await Promise.race([entered.promise, pending.then((outcome) => assert.fail("handler was not entered: " + outcome.error))]);
      if (stop === "cancel") assert.equal(app.session.cancel(), true);
      else write = false;
    } finally { release.resolve(); }
    const stopped = await pending;
    assert.equal(stopped.status, stop === "cancel" ? "cancelled" : "failed");
    assert.equal(stopped.possiblePartialEffects, true);
    assert.equal(stopped.report, undefined);
    assert.deepEqual(app.recorded, []);
    write = true;
    const next = await app.session.submit(translate + "green");
    assert.equal(next.status, "verified", next.error);
    assert.throws(() => late.commit(() => app.recorded.push("late")));
    assert.deepEqual(app.recorded, ["vert"]);
  });
}
