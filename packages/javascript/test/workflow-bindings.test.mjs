import assert from "node:assert/strict";
import test from "node:test";
import { AgentWorkflowSession, prepareSemanticWorkflow, prepareSemanticCatalog } from "@leselang/bridge";

const exact = (value) => typeof value === "bigint" ? String(value)
  : Array.isArray(value) ? `[${value.map(exact).join(",")}]`
  : value !== null && typeof value === "object" ? `{${Object.entries(value).map(([k, v]) => `${JSON.stringify(k)}:${exact(v)}`).join(",")}}`
  : JSON.stringify(value);
const literal = (kind, value) => kind === "none" ? { kind } : { kind, value };
const path = (name, scalar_type) => ({ kind: "scalar", path: [{ kind: "field", name }], scalar_type });
const predicate = (id, left, right) => ({ id, left, relation: { kind: "equal" }, right });
function catalog(options = {}) {
  const results = {
    read_count: literal("integer", options.integer ?? 3n),
    read_flag: literal("boolean", options.boolean ?? true),
    read_text: literal("string", options.string ?? 'observed "✓" \\ \nline'),
    no_result: literal("none"),
    write_text: literal("string", "saved"),
  };
  const operations = Object.entries(results).map(([id, result]) => ({
    name: `sample.${id}`, required_capability: "sample",
    parameters: id === "write_text" ? [{ name: "content", required: true, types: [{ kind: "scalar", name: "string" }] }] : [],
    result: { kind: "scalar", name: result.kind },
  }));
  const tasks = Object.entries(results).map(([id, result]) => ({
    id, summary: id, inputs: id === "write_text" ? [{ name: "content", scalar_type: "string" }] : [],
    execution: id === "write_text" ? "named_text" : "no_text",
    steps: [{ kind: "call", binding: "observed", operation: `sample.${id}`, arguments: id === "write_text"
      ? [{ name: "content", value: { kind: "input", name: "content" } }] : [] }],
    result: { kind: "binding", name: "observed" },
    verification: { kind: "binding_test", max_fuel: 100, result, require_verified: false,
      predicates: [predicate("proof", path("proof", "boolean"), { kind: "literal", value: literal("boolean", true) }),
        ...(id === "write_text" ? [predicate("saved", path("saved", "string"), { kind: "input", name: "content" })] : [])] },
  }));
  return { results, raw: exact({ schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "bindings.fixture",
    native_types: [], operations, semantics: { version: 1, operations: operations.map((operation) => ({
      operation: operation.name, summary: operation.name, effect: operation.name.endsWith("write_text") ? "write" : "read", completion: "value",
    })), tasks } }) };
}
const source = (task) => ({ task, arguments: {} });
const write = (content) => ({ task: "write_text", arguments: { content } });
const ref = (fromStep, format) => ({ fromStep, format });
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
async function until(predicate_) {
  for (let index = 0; index < 100 && !predicate_(); index++) await Promise.resolve();
  assert.ok(predicate_(), "expected stage reached");
}
function fixture(steps, options = {}) {
  const declaration = options.catalog ?? catalog();
  const calls = { describe: 0, execute: 0, verify: 0 };
  const plans = [], writes = [];
  const report = (plan) => {
    const result = declaration.results[plan.task];
    return exact({ schema_version: 1, profile: "bindings.fixture", kind: "binding_test", effects: 1, fuel_remaining: 99,
      result: { type: result.kind, value: result.kind === "none" ? null : result.value }, proof: true,
      ...(plan.task === "write_text" ? { saved: plan.arguments.content } : {}) });
  };
  const session = new AgentWorkflowSession({
    provider: { async plan() { return options.plan ?? prepareSemanticWorkflow(declaration.raw, steps); } },
    host: {
      async describe() { calls.describe++; return declaration.raw; },
      async execute(plan, signal) {
        calls.execute++; plans.push(plan);
        if (plan.task === "write_text") writes.push(plan.arguments.content);
        return options.execute ? options.execute(plan, signal, report(plan), calls.execute) : report(plan);
      },
      async verify(value, plan, signal) {
        calls.verify++;
        return options.verify ? options.verify(value, plan, signal, calls.verify) : true;
      },
    },
    onEvent(state) { options.event?.(state, session); },
  });
  return { session, calls, plans, writes, report, declaration };
}

test("helper preserves literal plan shape and owns symbolic backward bindings separately from source", () => {
  const c = catalog();
  const literalPlan = prepareSemanticWorkflow(c.raw, [source("read_count"), write("literal")]);
  assert.deepEqual(Object.keys(literalPlan), ["steps"]);
  assert.deepEqual(literalPlan.steps[1], prepareSemanticCatalog(c.raw).prepare("write_text", { content: "literal" }).plan);
  const input = [source("read_count"), write(ref(0, "decimal"))];
  const bound = prepareSemanticWorkflow(c.raw, input);
  input[1].arguments.content.fromStep = 99;
  assert.deepEqual(bound.bindings, [{ step: 1, argument: "content", fromStep: 0, format: "decimal" }]);
  assert.equal(bound.steps[1].arguments.content, "");
  assert.equal(bound.steps[1].source, literalPlan.steps[1].source);
  assert.ok(Object.isFrozen(bound.bindings) && Object.isFrozen(bound.bindings[0]));
  assert.ok(Object.isFrozen(bound.steps[1].arguments));
});

test("verified integer, Boolean and string reports become exact literal arguments", async () => {
  for (const [id, conversion, expected] of [
    ["read_count", "decimal", "3"], ["read_flag", "boolean", "true"], ["read_text", "string", 'observed "✓" \\ \nline'],
  ]) {
    const f = fixture([source(id), write(ref(0, conversion))]);
    const result = await f.session.submit("use the actual previous result");
    assert.equal(result.status, "verified");
    assert.deepEqual(f.writes, [expected]);
    assert.equal(result.plan.steps[1].arguments.content, "");
    assert.equal(result.steps[1].plan.arguments.content, expected);
    assert.equal(result.steps[1].plan.source.includes(expected), false);
    assert.equal(result.plan.steps[1].source, result.steps[1].plan.source);
    assert.deepEqual(f.calls, { describe: 3, execute: 2, verify: 2 });
  }
  const f = fixture([source("read_flag"), write(ref(0, "boolean"))], { catalog: catalog({ boolean: false }) });
  assert.equal((await f.session.submit("false as text")).status, "verified");
  assert.deepEqual(f.writes, ["false"]);
});

test("decimal binding preserves the exact u64 maximum instead of a rounded Number", async () => {
  const f = fixture([source("read_count"), write(ref(0, "decimal"))], { catalog: catalog({ integer: 18_446_744_073_709_551_615n }) });
  const result = await f.session.submit("forward u64 exactly");
  assert.equal(result.status, "verified");
  assert.deepEqual(f.writes, ["18446744073709551615"]);
  assert.match(result.steps[0].report, /"value":18446744073709551615/u);
  assert.doesNotMatch(result.steps[1].plan.arguments.content, /18446744073709552000/u);
});

test("a backward chain verifies each referenced source with its resolved arguments", async () => {
  const f = fixture([source("read_count"), write(ref(0, "decimal")), write(ref(1, "string")), write(ref(2, "string"))]);
  const result = await f.session.submit("count then save then forward save result");
  assert.equal(result.status, "verified");
  assert.deepEqual(f.writes, ["3", "saved", "saved"]);
  assert.deepEqual(result.steps.map((step) => step.status), ["verified", "verified", "verified", "verified"]);
  assert.equal(result.plan.bindings.length, 3);
});

test("the complete reference graph rejects wrong direction, formats and target shape before execution", async () => {
  const c = catalog();
  for (const steps of [
    [source("read_count"), write(ref(1, "decimal"))],
    [write(ref(1, "string")), source("read_text")],
    [write(ref(1, "string")), write(ref(0, "string"))],
    [source("read_count"), write(ref(7, "decimal"))],
    [source("read_count"), write(ref(-1, "decimal"))],
    [source("read_count"), write(ref(0.5, "decimal"))],
    [source("read_count"), write(ref("0", "decimal"))],
    [source("read_count"), write(ref(0, "string"))],
    [source("read_flag"), write(ref(0, "decimal"))],
    [source("no_result"), write(ref(0, "string"))],
    [source("read_count"), write({ fromStep: 0, format: "decimal", fallback: "3" })],
    [source("read_count"), { task: "write_text", arguments: { wrong: ref(0, "decimal") } }],
    [source("read_count"), { task: "no_result", arguments: { content: ref(0, "decimal") } }],
    [source("read_count"), { task: "write_text", arguments: { content: ref(0, "decimal"), extra: "x" } }],
  ]) {
    assert.throws(() => prepareSemanticWorkflow(c.raw, steps));
    const f = fixture(steps, { catalog: c });
    const result = await f.session.submit("reject the whole graph");
    assert.equal(result.status, "failed");
    assert.equal(result.steps.length, 0);
    assert.equal(result.possiblePartialEffects, false);
    assert.equal(f.calls.execute, 0);
  }
});

test("manual bindings cannot override literals, hide duplicate targets or change exact descriptor fields", async () => {
  const c = catalog();
  const base = prepareSemanticWorkflow(c.raw, [source("read_count"), write(""), write("")]);
  const binding = { step: 1, argument: "content", fromStep: 0, format: "decimal" };
  const invalid = [
    [binding, { ...binding }], [{ ...binding, value: "3" }], [{ ...binding, argument: "other" }],
    [{ ...binding, step: 0 }], [{ ...binding, step: 8 }], [{ ...binding, fromStep: 2 }],
    [{ ...binding, format: "integer" }], [{ ...binding, step: "1" }], [{ ...binding, fromStep: null }],
    [binding, { step: 2, argument: "content", fromStep: 2, format: "string" }],
    null, {}, new Array(1),
  ];
  for (const bindings of invalid) {
    const f = fixture([], { catalog: c, plan: { ...base, bindings } });
    const result = await f.session.submit("manual invalid binding");
    assert.equal(result.status, "failed");
    assert.equal(f.calls.execute, 0);
  }
  const literalPlan = prepareSemanticWorkflow(c.raw, [source("read_count"), write("must not replace")]);
  const f = fixture([], { catalog: c, plan: { ...literalPlan, bindings: [binding] } });
  assert.equal((await f.session.submit("literal collision")).status, "failed");
  assert.equal(f.calls.execute, 0);
});

test("accessors and coercion hooks in steps, references and bindings are never invoked", async () => {
  const c = catalog(); let hooks = 0;
  const get = () => { hooks++; throw new Error("must not call"); };
  for (const steps of [
    [source("read_count"), write({ get fromStep() { return get(); }, format: "decimal" })],
    [source("read_count"), write({ fromStep: 0, get format() { return get(); } })],
    [source("read_count"), write({ fromStep: 0, format: "decimal", toJSON: get })],
    Object.defineProperty([], "0", { get, enumerable: true }),
  ]) assert.throws(() => prepareSemanticWorkflow(c.raw, steps));
  const base = prepareSemanticWorkflow(c.raw, [source("read_count"), write("")]);
  const bindings = [{ step: 1, argument: "content", get fromStep() { return get(); }, format: "decimal" }];
  const f = fixture([], { catalog: c, plan: { ...base, bindings } });
  assert.equal((await f.session.submit("no binding getters")).status, "failed");
  assert.equal(hooks, 0);
});

test("a permissive application verifier cannot propagate malformed reports or false declared predicates", async () => {
  const mutations = [
    (report) => ({ ...report, result: { type: "integer", value: 4 } }),
    (report) => ({ ...report, result: { type: "string", value: "3" } }),
    (report) => ({ ...report, proof: false }),
    (report) => ({ ...report, proof: undefined }),
    (report) => ({ ...report, effects: 0 }),
    (report) => ({ ...report, fuel_remaining: 101 }),
    (report) => ({ ...report, profile: "other" }),
    (report) => ({ ...report, result: { type: "integer", value: Number.MAX_SAFE_INTEGER + 1 } }),
  ];
  for (const mutate of mutations) {
    const f = fixture([source("read_count"), write(ref(0, "decimal"))], { execute: async (_plan, _signal, raw) => JSON.stringify(mutate(JSON.parse(raw))) });
    const result = await f.session.submit("reject bad source evidence");
    assert.equal(result.status, "failed");
    assert.equal(result.error, "application_verification_failed");
    assert.equal(result.steps[0].status, "failed");
    assert.equal(result.possiblePartialEffects, true);
    assert.deepEqual(f.writes, []);
    assert.equal(f.calls.execute, 1);
  }
  for (const malformed of ["not JSON", '{"result":{},"result":{}}']) {
    const f = fixture([source("read_count"), write(ref(0, "decimal"))], { execute: async () => malformed });
    assert.equal((await f.session.submit("bad raw report")).status, "failed");
    assert.equal(f.calls.execute, 1);
  }
});

test("source application verification remains required and binding failure preserves prior mutations", async () => {
  const f = fixture([write("first mutation"), write(ref(0, "string"))], { verify: async () => false });
  const result = await f.session.submit("do not propagate application rejection");
  assert.equal(result.status, "failed");
  assert.equal(result.steps[0].status, "failed");
  assert.equal(result.possiblePartialEffects, true);
  assert.deepEqual(f.writes, ["first mutation"]);
  assert.equal(f.calls.execute, 1);
  const g = fixture([source("read_text"), write(ref(0, "string"))], { catalog: catalog({ string: "\0" }) });
  const refused = await g.session.submit("do not send NUL to next step");
  assert.equal(refused.status, "failed");
  assert.equal(refused.error, "invalid_workflow_result");
  assert.equal(refused.stoppedStep, 1);
  assert.equal(g.calls.execute, 1);
  assert.deepEqual(g.writes, []);
});

test("string bindings obey the existing UTF-8 input limit and keep source immutable", async () => {
  const content = "界".repeat(1365) + "x";
  const f = fixture([source("read_text"), write(ref(0, "string"))], { catalog: catalog({ string: content }) });
  const result = await f.session.submit("forward boundary literal");
  assert.equal(result.status, "verified");
  assert.equal(new TextEncoder().encode(f.writes[0]).length, 4096);
  const c = catalog();
  for (const value of ["界".repeat(1366), "\ud800", "\0"]) {
    assert.throws(() => prepareSemanticWorkflow(c.raw, [write(value)]));
  }
});

test("cancelling source verification waits for settlement and never resolves a later binding", async () => {
  const pending = deferred(); let signal;
  const f = fixture([source("read_count"), write(ref(0, "decimal"))], { verify: async (_report, _plan, forwarded) => {
    signal = forwarded; return pending.promise;
  } });
  const resultPromise = f.session.submit("cancel source verifier");
  await until(() => signal !== undefined);
  assert.equal(f.session.cancel(), true);
  assert.equal(signal.aborted, true);
  assert.equal(f.session.busy, true);
  await assert.rejects(f.session.submit("overlap"), /agent_task_pending/u);
  pending.resolve(true);
  const result = await resultPromise;
  assert.equal(result.status, "cancelled");
  assert.equal(result.steps[0].status, "cancelled");
  assert.equal(result.possiblePartialEffects, true);
  assert.deepEqual(f.writes, []);
});

test("cancelling between verified source and target preserves evidence without consuming its value", async () => {
  let cancelled = false;
  const f = fixture([write("kept"), write(ref(0, "string"))], { event(state, session) {
    if (!cancelled && state.completedSteps === 1) { cancelled = true; session.cancel(); }
  } });
  const result = await f.session.submit("cancel before using result");
  assert.equal(result.status, "cancelled");
  assert.equal(result.stoppedStep, 1);
  assert.equal(result.steps.length, 1);
  assert.equal(result.steps[0].status, "verified");
  assert.equal(result.possiblePartialEffects, true);
  assert.deepEqual(f.writes, ["kept"]);
});
