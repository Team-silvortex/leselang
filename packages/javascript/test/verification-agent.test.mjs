import assert from "node:assert/strict";
import test from "node:test";
import { AgentSession } from "@leselang/bridge";

function fixture(verification, detailed = true) {
  const calls = { execute: 0, verify: 0, detailed: 0 };
  const events = [];
  const host = {
    async describe() { return {}; },
    async execute() { calls.execute++; return { report: "application data" }; },
    verify() { calls.verify++; return verification; },
    ...(detailed ? { verifyDetailed(report, plan, signal) {
      calls.detailed++;
      return typeof verification === "function" ? verification(report, plan, signal) : verification;
    } } : {}),
  };
  const session = new AgentSession({
    host,
    provider: { async plan() { return { summary: "Read", source: "fn main() = true", arguments: {} }; } },
    onEvent(state) { events.push(state); },
  });
  return { session, calls, events };
}

test("detailed verification is used once and retains a frozen owned failure", async () => {
  const diagnostic = { code: "predicate_failed", predicate: "saved_content" };
  const result = { verified: false, diagnostic };
  const f = fixture(result);
  const outcome = await f.session.submit("verify application");
  diagnostic.predicate = "changed_later";
  result.verified = true;
  assert.equal(outcome.status, "failed");
  assert.equal(outcome.error, "application_verification_failed");
  assert.equal(outcome.possiblePartialEffects, true);
  assert.deepEqual(outcome.verificationFailure, { code: "predicate_failed", predicate: "saved_content" });
  assert.deepEqual(f.session.state.verificationFailure, outcome.verificationFailure);
  assert.ok(Object.isFrozen(outcome.verificationFailure));
  assert.equal(f.calls.verify, 0);
  assert.equal(f.calls.detailed, 1);
  assert.ok(f.events.slice(0, -1).every((state) => !Object.hasOwn(state, "verificationFailure")));
});

test("successful detailed and legacy Boolean verification keep their outcome shapes", async () => {
  for (const [verification, detailed, status] of [[{ verified: true }, true, "verified"], [true, false, "verified"], [false, false, "failed"]]) {
    const f = fixture(verification, detailed);
    const outcome = await f.session.submit("verify once");
    assert.equal(outcome.status, status);
    assert.equal(Object.hasOwn(outcome, "verificationFailure"), false);
    assert.equal(Object.hasOwn(f.session.state, "verificationFailure"), false);
    assert.equal(f.calls.verify, detailed ? 0 : 1);
    assert.equal(f.calls.detailed, detailed ? 1 : 0);
    if (status === "failed") assert.equal(outcome.error, "application_verification_failed");
  }
});

test("binding replacement diagnostics retain a completed attempt's partial effects", async () => {
  const diagnostic = { code: "application_bindings_changed" };
  const f = fixture({ verified: false, diagnostic });
  const outcome = await f.session.submit("verify the same mounted controls");
  diagnostic.code = "report_mismatch";
  assert.equal(outcome.status, "failed");
  assert.equal(outcome.error, "application_verification_failed");
  assert.equal(outcome.possiblePartialEffects, true);
  assert.deepEqual(outcome.verificationFailure, { code: "application_bindings_changed" });
  assert.ok(Object.isFrozen(outcome.verificationFailure));
  assert.deepEqual({ ...outcome.report }, { report: "application data" });
  assert.equal(f.calls.verify, 0);
  assert.equal(f.calls.detailed, 1);
});

test("execution report diagnostics remain ordinary failed outcomes with bounded detail", async () => {
  const codes = ["execution_failed", "execution_object_unavailable", "execution_fuel_exhausted",
    "execution_deadline_exceeded", "execution_scope_closed", "execution_cancelled"];
  for (const code of codes) {
    const f = fixture({ verified: false, diagnostic: { code } });
    const outcome = await f.session.submit("check terminal execution evidence");
    assert.equal(outcome.status, "failed", "a reported cancellation is not the local AbortSignal");
    assert.equal(outcome.error, "application_verification_failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(outcome.verificationFailure, { code });
    assert.deepEqual(f.session.state.verificationFailure, { code });
    assert.deepEqual({ ...outcome.report }, { report: "application data" });
    assert.equal(f.calls.verify, 0);
    assert.equal(f.calls.detailed, 1);
    const malformed = fixture({ verified: false, diagnostic: { code, predicate: "not_an_execution_field" } });
    assert.deepEqual((await malformed.session.submit("reject extra detail")).verificationFailure,
      { code: "invalid_verification_result" });
  }
});

test("host detail records cannot grant success through accessors, coercion or malformed shapes", async () => {
  let getters = 0;
  const getter = () => { getters++; return true; };
  const symbol = { verified: true, [Symbol("extra")]: true };
  const malformed = [
    true, false, null, undefined, [], "true", { verified: 1 }, {},
    { verified: true, diagnostic: { code: "predicate_failed", predicate: "check" } },
    { verified: true, extra: true }, { verified: false }, { verified: false, diagnostic: { code: "unknown" } },
    { verified: false, diagnostic: { code: "result_mismatch", predicate: "check" } },
    { verified: false, diagnostic: { code: "application_bindings_changed", predicate: "check" } },
    { verified: false, diagnostic: { code: "predicate_failed" } },
    { verified: false, diagnostic: { code: "predicate_failed", predicate: "x".repeat(65) } },
    { verified: false, diagnostic: { code: "predicate_failed", predicate: "content of message" } },
    { verified: false, diagnostic: { code: "predicate_failed", predicate: "_lese_hidden" } },
    { verified: false, diagnostic: { code: "result_mismatch", expected: "secret" } },
    Object.assign(Object.create({ inherited: true }), { verified: true }), symbol,
    { get verified() { return getter(); } },
    { verified: false, get diagnostic() { return getter(); } },
    { verified: false, diagnostic: { get code() { return getter(); } } },
    { verified: true, toJSON: getter },
    { get then() { getters++; return (resolve) => resolve({ verified: true }); } },
    { then(resolve) { getters++; resolve({ verified: true }); } },
    Object.defineProperty({}, "verified", { value: true }),
  ];
  for (const value of malformed) {
    const f = fixture(value);
    const outcome = await f.session.submit("reject malformed verification");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.deepEqual(outcome.verificationFailure, { code: "invalid_verification_result" });
    assert.equal(f.calls.verify, 0);
  }
  assert.equal(getters, 0);
});

test("cancelled detailed verification cannot publish a late result or diagnostic", async () => {
  let resolve;
  const pending = new Promise((yes) => { resolve = yes; });
  const f = fixture(() => pending);
  const execution = f.session.submit("cancel verification");
  for (let index = 0; index < 20 && !f.calls.detailed; index++) await Promise.resolve();
  assert.equal(f.calls.detailed, 1);
  assert.equal(f.session.cancel(), true);
  assert.equal(f.session.busy, true);
  await assert.rejects(f.session.submit("overlap"), /agent_task_pending/);
  resolve({ verified: false, diagnostic: { code: "after_observation_mismatch" } });
  const outcome = await execution;
  assert.equal(outcome.status, "cancelled");
  assert.equal(outcome.possiblePartialEffects, true);
  assert.equal(Object.hasOwn(outcome, "verificationFailure"), false);
  assert.equal(Object.hasOwn(f.session.state, "verificationFailure"), false);
  assert.equal(f.calls.verify, 0);
});

test("a thrown detailed verifier preserves its original failure without inventing diagnostic data", async () => {
  const f = fixture(() => { throw new Error("host verification unavailable"); });
  const outcome = await f.session.submit("verify");
  assert.equal(outcome.error, "host verification unavailable");
  assert.equal(outcome.possiblePartialEffects, true);
  assert.equal(Object.hasOwn(outcome, "verificationFailure"), false);
  assert.equal(f.calls.verify, 0);
});
