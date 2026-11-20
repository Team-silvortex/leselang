import assert from "node:assert/strict";
import test from "node:test";
import { AgentWorkflowSession, prepareSemanticWorkflow } from "@leselang/bridge";

const exact = (value) => typeof value === "bigint" ? String(value)
  : Array.isArray(value) ? `[${value.map(exact).join(",")}]`
  : value !== null && typeof value === "object" ? `{${Object.entries(value).map(([k, v]) => `${JSON.stringify(k)}:${exact(v)}`).join(",")}}`
  : JSON.stringify(value);
const field = (name) => [{ kind: "field", name }];
const scalar = (name, scalar_type) => ({ kind: "scalar", path: field(name), scalar_type });
const literal = (kind, value) => ({ kind: "literal", value: { kind, value } });
const equal = (id, left, right) => ({ id, left, relation: { kind: "equal" }, right });
function catalog() {
  const kinds = { read_text: "string", read_count: "integer", read_flag: "boolean", no_result: "none", write_text: "string" };
  const operations = Object.entries(kinds).map(([id, kind]) => ({
    name: `sample.${id}`, required_capability: "sample",
    parameters: id === "write_text" ? [{ name: "content", required: true, types: [{ kind: "scalar", name: "string" }] }] : [],
    result: { kind: "scalar", name: kind },
  }));
  return exact({ schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "requirements.fixture",
    native_types: [], operations, semantics: { version: 2,
      operations: operations.map((op) => ({ operation: op.name, summary: op.name,
        effect: op.name.endsWith("write_text") ? "write" : "read", completion: "value" })),
      tasks: Object.entries(kinds).map(([id, kind]) => ({
        id, summary: id, inputs: id === "write_text" ? [{ name: "content", scalar_type: "string" }] : [],
        execution: id === "write_text" ? "named_text" : "no_text",
        steps: [{ kind: "call", binding: "observed", operation: `sample.${id}`,
          arguments: id === "write_text" ? [{ name: "content", value: { kind: "input", name: "content" } }] : [] }],
        result: { kind: "binding", name: "observed" },
        verification: { kind: "requirement_report", max_fuel: 100, require_verified: false,
          result: kind === "none" ? { kind: "none" } : id === "write_text" ? { kind: "string", value: "saved" }
            : { kind: "observed", scalar_type: kind, path: field("observed_value") },
          predicates: [equal("proof", scalar("proof", "boolean"), literal("boolean", true)),
            ...(id === "write_text" ? [equal("saved", scalar("saved", "string"), { kind: "input", name: "content" })] : [])],
        },
      })),
    },
  });
}
const read = (task = "read_text") => ({ task, arguments: {} });
const requireResult = (fromStep = 0, format = "string", equals = "expected") => ({ fromStep, format, equals });
const write = (requires, content = "replacement") => ({ task: "write_text", arguments: { content }, ...(requires ? { requires } : {}) });
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
async function until(predicate) {
  for (let index = 0; index < 100 && !predicate(); index++) await Promise.resolve();
  assert.ok(predicate(), "expected stage reached");
}
function fixture(steps = [read(), write(requireResult())], options = {}) {
  const raw = options.catalog ?? catalog();
  const calls = { describe: 0, execute: 0, verify: 0 };
  const writes = [], plans = [];
  const values = { read_text: options.string ?? "expected", read_count: options.integer ?? 3n,
    read_flag: options.boolean ?? true, no_result: null, write_text: "saved" };
  const kinds = { read_text: "string", read_count: "integer", read_flag: "boolean", no_result: "none", write_text: "string" };
  const report = (plan) => exact({ schema_version: 1, profile: "requirements.fixture", kind: "requirement_report",
    effects: 1, fuel_remaining: 99, result: { type: kinds[plan.task], value: values[plan.task] },
    observed_value: values[plan.task], proof: true, ...(plan.task === "write_text" ? { saved: plan.arguments.content } : {}) });
  const session = new AgentWorkflowSession({
    provider: { async plan() { return options.plan ?? prepareSemanticWorkflow(raw, steps); } },
    host: {
      async describe(signal) { calls.describe++; return options.describe ? options.describe(raw, calls.describe, signal) : raw; },
      async execute(plan, signal) {
        calls.execute++; plans.push(plan);
        if (plan.task === "write_text") writes.push(plan.arguments.content);
        return options.execute ? options.execute(plan, signal, report(plan), calls.execute) : report(plan);
      },
      async verify(value, plan, signal) {
        calls.verify++; return options.verify ? options.verify(value, plan, signal, calls.verify) : true;
      },
    },
    onEvent(state) { options.event?.(state, session); },
  });
  return { session, calls, writes, plans, raw, report };
}

test("requirements are separate owned data; an unconditional plan and outcome retain their shape", async () => {
  const raw = catalog(), steps = [read(), write(requireResult())];
  const prepared = prepareSemanticWorkflow(raw, steps);
  assert.deepEqual(prepared.requirements, [{ step: 1, fromStep: 0, format: "string", equals: "expected" }]);
  assert.ok(Object.isFrozen(prepared.requirements) && Object.isFrozen(prepared.requirements[0]));
  steps[1].requires.equals = "mutated";
  assert.equal(prepared.requirements[0].equals, "expected");
  const plain = prepareSemanticWorkflow(raw, [read(), write(undefined)]);
  assert.deepEqual(Object.keys(plain), ["steps"]);
  assert.equal(prepared.steps[1].source, plain.steps[1].source);
  assert.deepEqual(prepared.steps[1].arguments, plain.steps[1].arguments);
  const f = fixture([read(), write(undefined)]);
  const outcome = await f.session.submit("ordinary workflow");
  assert.equal(outcome.status, "verified");
  assert.equal("requirements" in outcome, false);
});

test("v1 fixed scalar contracts admit requirements without changing their verification", async () => {
  const declared = JSON.parse(catalog());
  declared.semantics.version = 1;
  const values = { read_text: "expected", read_count: 3, read_flag: true };
  for (const task of declared.semantics.tasks) {
    if (Object.hasOwn(values, task.id)) task.verification.result = {
      kind: task.verification.result.scalar_type, value: values[task.id],
    };
  }
  const raw = exact(declared);
  for (const [source, format, expected, different] of [
    ["read_text", "string", "expected", "different"],
    ["read_count", "decimal", "3", "4"],
    ["read_flag", "boolean", "true", "false"],
  ]) {
    for (const equals of [expected, different]) {
      const f = fixture([read(source), write(requireResult(0, format, equals))], { catalog: raw });
      const outcome = await f.session.submit("v1 fixed contract compatibility");
      assert.equal(outcome.status, equals === expected ? "verified" : "failed");
      assert.equal(outcome.requirements[0].actual, expected);
      assert.equal(outcome.requirements[0].satisfied, equals === expected);
      assert.equal(f.calls.execute, equals === expected ? 2 : 1);
      if (equals !== expected) assert.equal(outcome.error, "workflow_requirement_not_met");
    }
  }
  const forged = fixture([read(), write(requireResult(0, "string", "forged"))], { catalog: raw, string: "forged" });
  const outcome = await forged.session.submit("v1 fixed result cannot be replaced by a matching condition");
  assert.equal(outcome.error, "application_verification_failed");
  assert.equal(outcome.requirements, undefined);
  assert.equal(forged.calls.execute, 1);
  assert.equal(forged.calls.verify, 0);
  assert.deepEqual(forged.writes, []);
});

test("the same declaration compares actual observed results and stops before a mismatched target", async () => {
  for (const actual of ["expected", "changed externally"]) {
    const f = fixture(undefined, { string: actual });
    const outcome = await f.session.submit("replace only the expected text");
    const satisfied = actual === "expected";
    assert.equal(outcome.status, satisfied ? "verified" : "failed");
    assert.deepEqual(outcome.requirements, [{ step: 1, fromStep: 0, format: "string", equals: "expected", actual, satisfied }]);
    assert.ok(Object.isFrozen(outcome.requirements) && Object.isFrozen(outcome.requirements[0]));
    assert.deepEqual(f.writes, satisfied ? ["replacement"] : []);
    assert.deepEqual(f.calls, { describe: 3, execute: satisfied ? 2 : 1, verify: satisfied ? 2 : 1 });
    assert.equal(outcome.steps[0].status, "verified");
    assert.equal(outcome.steps.length, 2);
    if (!satisfied) {
      assert.equal(outcome.error, "workflow_requirement_not_met");
      assert.equal(outcome.stoppedStep, 1);
      assert.equal(outcome.steps[1].status, "failed");
      assert.equal(outcome.steps[1].possiblePartialEffects, false);
      assert.equal("plan" in outcome.steps[1], false);
      assert.equal("report" in outcome.steps[1], false);
      assert.equal(outcome.possiblePartialEffects, true);
    }
  }
});

test("requirements compare exact u64, Boolean, empty and UTF-8 boundary string values", async () => {
  for (const [task, format, equals, options] of [
    ["read_count", "decimal", "18446744073709551615", { integer: 18_446_744_073_709_551_615n }],
    ["read_count", "decimal", "0", { integer: 0n }],
    ["read_flag", "boolean", "false", { boolean: false }],
    ["read_flag", "boolean", "true", { boolean: true }],
    ["read_text", "string", "", { string: "" }],
    ["read_text", "string", "界".repeat(1365) + "x", { string: "界".repeat(1365) + "x" }],
    ["read_text", "string", '{"fromStep":7,"format":"decimal"}', { string: '{"fromStep":7,"format":"decimal"}' }],
  ]) {
    const f = fixture([read(task), write(requireResult(0, format, equals))], options);
    const outcome = await f.session.submit("exact comparison");
    assert.equal(outcome.status, "verified");
    assert.equal(outcome.requirements[0].actual, equals);
    assert.equal(outcome.requirements[0].satisfied, true);
  }
  const f = fixture([read("read_count"), write(requireResult(0, "decimal", "18446744073709551614"))], { integer: 18_446_744_073_709_551_615n });
  assert.equal((await f.session.submit("no integer rounding")).error, "workflow_requirement_not_met");
  assert.equal(f.calls.execute, 1);
});

test("bound inputs and requirements share verified results and use the resolved source arguments", async () => {
  const steps = [read(), write(requireResult(), { fromStep: 0, format: "string" }),
    write(requireResult(1, "string", "saved"), { fromStep: 1, format: "string" })];
  const f = fixture(steps);
  const outcome = await f.session.submit("check then bind each result");
  assert.equal(outcome.status, "verified");
  assert.deepEqual(f.writes, ["expected", "saved"]);
  assert.equal(outcome.requirements.length, 2);
  assert.deepEqual(outcome.plan.steps.map((step) => step.arguments.content), [undefined, "", ""]);
  assert.deepEqual(outcome.steps.map((step) => step.plan.arguments.content), [undefined, "expected", "saved"]);
});

test("every malformed or ill-typed requirement is rejected before executing any step", async () => {
  const invalid = [
    requireResult(1), requireResult(2), requireResult(-1), requireResult(0.5), requireResult("0"),
    requireResult(0, "decimal", "3"), requireResult(0, "boolean", "true"), requireResult(0, "auto"),
    requireResult(0, "string", 1), requireResult(0, "string", "\0"), requireResult(0, "string", "\ud800"),
    requireResult(0, "string", "界".repeat(1366)),
    { ...requireResult(), fallback: true }, { fromStep: 0, format: "string" }, null, [],
  ];
  for (const requires of invalid) {
    const steps = [read(), { ...write(undefined), requires }];
    assert.throws(() => prepareSemanticWorkflow(catalog(), steps));
    const f = fixture(steps);
    const outcome = await f.session.submit("reject complete plan");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.steps.length, 0);
    assert.equal(outcome.possiblePartialEffects, false);
    assert.equal("requirements" in outcome, false);
    assert.equal(f.calls.execute, 0);
  }
  for (const equals of ["", "01", "+1", "-0", " 1", "1 ", "1.0", "1e0", "18446744073709551616", "9".repeat(100)]) {
    assert.throws(() => prepareSemanticWorkflow(catalog(), [read("read_count"), write(requireResult(0, "decimal", equals))]));
  }
  for (const equals of ["True", "FALSE", "0", "1", " false"]) {
    assert.throws(() => prepareSemanticWorkflow(catalog(), [read("read_flag"), write(requireResult(0, "boolean", equals))]));
  }
  assert.throws(() => prepareSemanticWorkflow(catalog(), [read("no_result"), write(requireResult())]));
});

test("raw provider requirements cannot hide extra fields, duplicate targets or an invalid later condition", async () => {
  const base = prepareSemanticWorkflow(catalog(), [read(), write(undefined), write(undefined)]);
  const requirement = { step: 1, ...requireResult() };
  for (const requirements of [
    [{ ...requirement, actual: "expected", satisfied: true }], [requirement, requirement],
    [{ ...requirement, step: 0 }], [{ ...requirement, step: 8 }], [{ ...requirement, step: "1" }],
    [{ ...requirement, fromStep: 1 }], [{ ...requirement, equals: "\0" }],
    [requirement, { ...requirement, step: 2, fromStep: 2 }],
    Array(9).fill(requirement), new Array(1), null, {},
  ]) {
    const f = fixture([], { plan: { ...base, requirements }, string: "would not match first condition" });
    const outcome = await f.session.submit("admit the entire manual proposal");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.steps.length, 0);
    assert.equal(f.calls.execute, 0);
  }
  const badLastStep = { ...base, steps: [...base.steps.slice(0, 2), { ...base.steps[2], source: "forged" }], requirements: [requirement] };
  const f = fixture([], { plan: badLastStep, string: "mismatch" });
  assert.equal((await f.session.submit("still admit last program")).error, "invalid_workflow_source");
  assert.equal(f.calls.execute, 0);
});

test("requirement accessors, symbols, holes and coercion hooks are never invoked", async () => {
  let hooks = 0;
  const hook = () => { hooks++; throw new Error("do not invoke"); };
  const base = prepareSemanticWorkflow(catalog(), [read(), write(undefined)]);
  for (const requirement of [
    { step: 1, ...requireResult(), get equals() { return hook(); } },
    { step: 1, ...requireResult(), toJSON: hook },
    { step: 1, ...requireResult(), [Symbol("extra")]: true },
    Object.defineProperty({ step: 1, fromStep: 0, format: "string" }, "equals", { value: "expected" }),
  ]) {
    const f = fixture([], { plan: { ...base, requirements: [requirement] } });
    assert.equal((await f.session.submit("no hooks")).status, "failed");
    assert.equal(f.calls.execute, 0);
  }
  const getters = Object.defineProperty([], "0", { get: hook, enumerable: true });
  const f = fixture([], { plan: { ...base, requirements: getters } });
  assert.equal((await f.session.submit("no array getter")).status, "failed");
  assert.throws(() => prepareSemanticWorkflow(catalog(), [read(), { ...write(undefined), get requires() { return hook(); } }]));
  assert.equal(hooks, 0);
});

test("a permissive host cannot satisfy requirements with forged, stale or malformed source evidence", async () => {
  const corrupt = [
    (report) => ({ ...report, observed_value: "stale" }),
    (report) => ({ ...report, proof: false }),
    (report) => ({ ...report, result: { type: "integer", value: 3 } }),
    (report) => ({ ...report, effects: 0 }),
    (report) => ({ ...report, fuel_remaining: 101 }),
    (report) => ({ ...report, profile: "another application" }),
  ];
  for (const mutation of corrupt) {
    const f = fixture(undefined, { execute: async (_plan, _signal, raw) => exact(mutation(JSON.parse(raw))) });
    const outcome = await f.session.submit("require independent source validation");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.equal("requirements" in outcome, false);
    assert.equal(f.calls.execute, 1);
    assert.equal(f.calls.verify, 0);
    assert.deepEqual(f.writes, []);
  }
  for (const raw of ["not JSON", '{"result":{},"result":{}}']) {
    const f = fixture(undefined, { execute: async () => raw });
    const outcome = await f.session.submit("malformed source");
    assert.equal(outcome.status, "failed");
    assert.equal("requirements" in outcome, false);
    assert.equal(f.calls.execute, 1);
  }
});

test("unsafe numeric results and values unfit for an exact comparison never become decisions", async () => {
  const f = fixture([read("read_count"), write(requireResult(0, "decimal", "9007199254740992"))], {
    integer: 9_007_199_254_740_992n,
    execute: async (_plan, _signal, raw) => JSON.parse(raw),
  });
  const numeric = await f.session.submit("do not round an object report");
  assert.equal(numeric.status, "failed");
  assert.equal("requirements" in numeric, false);
  assert.equal(f.calls.execute, 1);
  const nul = fixture(undefined, { string: "\0" });
  const outcome = await nul.session.submit("do not normalize an invalid result");
  assert.equal(outcome.error, "invalid_workflow_result");
  assert.equal(outcome.stoppedStep, 1);
  assert.equal("requirements" in outcome, false);
  assert.equal(nul.calls.execute, 1);
  const extra = fixture(undefined, { execute: async (_plan, _signal, raw) => {
    const report = JSON.parse(raw);
    report.result.satisfied = true;
    return exact(report);
  } });
  const ambiguous = await extra.session.submit("do not accept result verification claims");
  assert.equal(ambiguous.error, "invalid_workflow_result");
  assert.equal(ambiguous.steps[0].status, "verified");
  assert.equal("requirements" in ambiguous, false);
  assert.equal(extra.calls.execute, 1);
});

test("application rejection and execution failure stop before comparison and preserve earlier mutations", async () => {
  for (const options of [
    { verify: async () => false },
    { verify: async () => { throw new Error("fresh_state_failed"); } },
    { execute: async () => { throw new Error("execution_failed"); } },
  ]) {
    const f = fixture([write(undefined, "already committed"), write(requireResult(0, "string", "saved"))], options);
    const outcome = await f.session.submit("no use of unverified outcomes");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.steps.length, 1);
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal("requirements" in outcome, false);
    assert.deepEqual(f.writes, ["already committed"]);
  }
});

test("a later mismatch preserves earlier decisions and effects without attempting remaining steps", async () => {
  const f = fixture([read(), write(requireResult(), "committed"), write(requireResult(1, "string", "not saved")), write(undefined)]);
  const outcome = await f.session.submit("do not roll back or continue");
  assert.equal(outcome.error, "workflow_requirement_not_met");
  assert.equal(outcome.stoppedStep, 2);
  assert.equal(outcome.steps.length, 3);
  assert.deepEqual(outcome.requirements.map((item) => item.satisfied), [true, false]);
  assert.deepEqual(f.writes, ["committed"]);
  assert.equal(outcome.possiblePartialEffects, true);
});

test("source cancellation waits for late execute and verify settlement without comparing or writing", async () => {
  for (const stage of ["execute", "verify"]) {
    const pending = deferred(); let signal, late;
    const f = fixture(undefined, { [stage]: async (...args) => {
      signal = stage === "execute" ? args[1] : args[2];
      late = stage === "execute" ? args[2] : true;
      return pending.promise;
    } });
    const promise = f.session.submit("cancel pending source");
    await until(() => signal !== undefined);
    assert.equal(f.session.cancel(), true);
    assert.equal(signal.aborted, true);
    assert.equal(f.session.busy, true);
    await assert.rejects(f.session.submit("overlap"), /agent_task_pending/u);
    pending.resolve(late);
    const outcome = await promise;
    assert.equal(outcome.status, "cancelled");
    assert.equal(outcome.steps[0].status, "cancelled");
    assert.equal("requirements" in outcome, false);
    assert.deepEqual(f.writes, []);
    assert.equal(f.session.busy, false);
  }
});

test("cancellation between steps and during fresh target description prevents comparison", async () => {
  const between = fixture(undefined, { event(state, session) { if (state.completedSteps === 1) session.cancel(); } });
  const first = await between.session.submit("stop before requirement");
  assert.equal(first.status, "cancelled");
  assert.equal(first.stoppedStep, 1);
  assert.equal("requirements" in first, false);
  const pending = deferred(); let signal, raw;
  const f = fixture(undefined, { describe: async (value, count, forwarded) => {
    if (count !== 3) return value;
    raw = value; signal = forwarded; return pending.promise;
  } });
  const promise = f.session.submit("cancel target description");
  await until(() => signal !== undefined);
  f.session.cancel();
  assert.equal(signal.aborted, true);
  pending.resolve(raw);
  const outcome = await promise;
  assert.equal(outcome.status, "cancelled");
  assert.equal("requirements" in outcome, false);
  assert.equal(f.calls.execute, 1);
});

test("catalog changes reject the target before comparing even an otherwise matching result", async () => {
  const f = fixture(undefined, { describe: async (raw, count) => count === 3 ? `${raw} ` : raw });
  const outcome = await f.session.submit("fresh contract required");
  assert.equal(outcome.error, "workflow_catalog_changed");
  assert.equal(outcome.stoppedStep, 1);
  assert.equal("requirements" in outcome, false);
  assert.equal(f.calls.execute, 1);
});

test("a completed comparison never claims execution when cancellation prevents the target", async () => {
  const f = fixture(undefined, { event(state, session) {
    if (state.stepIndex === 1 && state.phase === "executing") session.cancel();
  } });
  const outcome = await f.session.submit("cancel after comparing");
  assert.equal(outcome.status, "cancelled");
  assert.equal(outcome.requirements[0].satisfied, true);
  assert.equal(outcome.steps[1].status, "cancelled");
  assert.equal(outcome.steps[1].possiblePartialEffects, false);
  assert.equal("report" in outcome.steps[1], false);
  assert.equal(f.calls.execute, 1);
});

test("provider mutation cannot rewrite an admitted requirement while source work is outstanding", async () => {
  const prepared = prepareSemanticWorkflow(catalog(), [read(), write(requireResult())]);
  const plan = { steps: prepared.steps.map((step) => ({ ...step, arguments: { ...step.arguments } })),
    requirements: prepared.requirements.map((requirement) => ({ ...requirement })) };
  const pending = deferred(); let report;
  const f = fixture([], { plan, execute: async (_plan, _signal, raw, count) => {
    if (count !== 1) return raw;
    report = raw; return pending.promise;
  } });
  const promise = f.session.submit("own all requirements before awaiting execution");
  await until(() => report !== undefined);
  plan.requirements[0].equals = "mutated";
  plan.requirements[0].fromStep = 1;
  plan.steps[1].arguments.content = "mutated payload";
  pending.resolve(report);
  const outcome = await promise;
  assert.equal(outcome.status, "verified");
  assert.equal(outcome.requirements[0].equals, "expected");
  assert.deepEqual(f.writes, ["replacement"]);
});

test("a requirement compares the selected historical observation rather than rereading or locking current state", async () => {
  let current = "A";
  const f = fixture([read(), write(undefined, "B"), write(requireResult(0, "string", "A"), "C")], {
    execute: async (plan, _signal, raw) => {
      if (plan.task === "read_text") {
        const report = JSON.parse(raw);
        report.result.value = current;
        report.observed_value = current;
        return exact(report);
      }
      current = plan.arguments.content;
      return raw;
    },
  });
  const outcome = await f.session.submit("condition on the earlier read");
  assert.equal(outcome.status, "verified");
  assert.equal(outcome.requirements[0].actual, "A");
  assert.equal(outcome.requirements[0].satisfied, true);
  assert.deepEqual(f.plans.map((plan) => plan.task), ["read_text", "write_text", "write_text"]);
  assert.deepEqual(f.writes, ["B", "C"]);
  assert.equal(current, "C");
});
