import assert from "node:assert/strict";
import test from "node:test";
import {
  AgentWorkflowSession, createModelProvider, createModelWorkflowProvider,
  prepareSemanticCatalog, prepareSemanticWorkflow,
} from "@leselang/bridge";

const exact = (value) => typeof value === "bigint" ? String(value)
  : Array.isArray(value) ? `[${value.map(exact).join(",")}]`
  : value !== null && typeof value === "object" ? `{${Object.entries(value).map(([key, item]) => `${JSON.stringify(key)}:${exact(item)}`).join(",")}}`
  : JSON.stringify(value);
const field = (name) => ({ kind: "field", name });
const observedPath = () => [field("observations"), field("rows"), { kind: "match", field: "name", value: "selected" }, field("value")];
function declaration(kind = "string") {
  const operation = (name, type, input) => ({ name, required_capability: "records",
    parameters: input ? [{ name: "message", required: true, types: [{ kind: "scalar", name: "string" }] }] : [],
    result: { kind: "scalar", name: type } });
  const proof = { id: "fresh_evidence", left: { kind: "scalar", scalar_type: "boolean", path: [field("proof")] },
    relation: { kind: "equal" }, right: { kind: "literal", value: { kind: "boolean", value: true } } };
  const task = (id, operation, type, input) => ({
    id, summary: id, inputs: input ? [{ name: "message", scalar_type: "string" }] : [], execution: input ? "named_text" : "no_text",
    steps: [{ kind: "call", binding: "observed_value", operation,
      arguments: input ? [{ name: "message", value: { kind: "input", name: "message" } }] : [] }],
    result: { kind: "binding", name: "observed_value" },
    verification: { kind: "observed_run", max_fuel: 100, require_verified: false,
      result: input ? { kind: "string", value: "saved" } : { kind: "observed", scalar_type: type, path: observedPath() },
      predicates: [proof, ...(input ? [{ id: "saved_message", left: { kind: "scalar", scalar_type: "string", path: [field("saved")] },
        relation: { kind: "equal" }, right: { kind: "input", name: "message" } }] : [])] },
  });
  return { schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "records.observed",
    native_types: [], operations: [operation("records.read", kind), operation("records.save", "string", true)],
    semantics: { version: 2, operations: [
      { operation: "records.read", summary: "Read the current value", effect: "read", completion: "value" },
      { operation: "records.save", summary: "Save supplied text", effect: "write", completion: "value" },
    ], tasks: [task("read_current", "records.read", kind), task("save_current", "records.save", "string", true)] } };
}
const report = (kind, value) => ({ schema_version: 1, profile: "records.observed", kind: "observed_run", effects: 1, fuel_remaining: 99,
  result: { type: kind, value }, proof: true, observations: { rows: [{ name: "selected", value }] } });
const steps = (format = "string") => [{ task: "read_current", arguments: {} },
  { task: "save_current", arguments: { message: { fromStep: 0, format } } }];
function deferred() { let resolve; const promise = new Promise((yes) => { resolve = yes; }); return { promise, resolve }; }
async function until(predicate) {
  for (let index = 0; index < 100 && !predicate(); index++) await Promise.resolve();
  assert.ok(predicate(), "expected workflow stage reached");
}
function workflow(kind, values, options = {}) {
  const raw = exact(declaration(kind));
  const format = kind === "integer" ? "decimal" : kind;
  const writes = [], plans = [];
  let reads = 0;
  const session = new AgentWorkflowSession({
    provider: { async plan() { return prepareSemanticWorkflow(raw, steps(format)); } },
    host: {
      async describe() { return raw; },
      async execute(plan, signal) {
        plans.push(plan);
        if (plan.task === "read_current") {
          const result = report(kind, values[reads++]);
          return options.read ? options.read(result, signal) : exact(result);
        }
        writes.push(plan.arguments.message);
        return exact({ ...report("string", "saved"), saved: plan.arguments.message });
      },
      async verify(value, plan, signal) { return options.verify ? options.verify(value, plan, signal) : true; },
    },
    onEvent(state) { options.event?.(state, session); },
  });
  return { session, writes, plans, raw };
}

test("one v2 declaration verifies distinct current values without catalog or source changes", () => {
  for (const [kind, values] of [["string", ["first ✓", 'second "quoted" \\ \nline', ""]],
    ["boolean", [true, false]], ["integer", [0n, 9n, 18_446_744_073_709_551_615n]]]) {
    const raw = exact(declaration(kind));
    const prepared = prepareSemanticCatalog(raw).prepare("read_current", {});
    for (const value of values) assert.equal(prepared.verify(exact(report(kind, value))), true);
    assert.equal(prepareSemanticCatalog(raw).prepare("read_current", {}).plan.source, prepared.plan.source);
    assert.equal(prepared.expectedEffects, 1);
    assert.deepEqual(prepared.checks, ["fresh_evidence"]);
  }
});

test("v2 fixed results retain literal equality and v1 refuses observed contracts", () => {
  const value = declaration();
  const fixed = prepareSemanticCatalog(value).prepare("save_current", { message: "text" });
  const evidence = { ...report("string", "saved"), saved: "text" };
  assert.equal(fixed.verify(evidence), true);
  evidence.result.value = "other";
  assert.equal(fixed.verify(evidence), false);
  for (const version of [1, 0, 3, 999, "2", 2.5]) {
    const invalid = declaration(); invalid.semantics.version = version;
    assert.throws(() => prepareSemanticCatalog(invalid));
  }
  const v1 = declaration(); v1.semantics.version = 1;
  v1.semantics.tasks[0].verification.result = { kind: "string", value: "fixed" };
  assert.equal(prepareSemanticCatalog(v1).prepare("read_current", {}).verify(report("string", "fixed")), true);
  assert.equal(prepareSemanticCatalog(v1).prepare("read_current", {}).verify(report("string", "different")), false);
});

test("observed declarations require strict scalar type and an independent bounded path", () => {
  const malformed = [
    { kind: "observed", scalar_type: "none", path: observedPath() },
    { kind: "observed", scalar_type: "optional_string", path: observedPath() },
    { kind: "observed", scalar_type: "string_list", path: observedPath() },
    { kind: "observed", scalar_type: "native:Value", path: observedPath() },
    { kind: "observed", scalar_type: "string", path: [] },
    { kind: "observed", scalar_type: "string", path: Array(9).fill(field("x")) },
    { kind: "observed", scalar_type: "string", path: [field("result"), field("value")] },
    { kind: "observed", scalar_type: "string", path: [{ kind: "match", field: "name", value: "selected" }] },
    { kind: "observed", scalar_type: "string", path: [field("")] },
    { kind: "observed", scalar_type: "string", path: [field("x".repeat(129))] },
    { kind: "observed", scalar_type: "string", path: [field("observations"), { kind: "length" }] },
    { kind: "observed", scalar_type: "string", path: [field("observations"), { kind: "field", name: "value", extra: true }] },
    { kind: "observed", scalar_type: "string", path: [field("observations"), { kind: "match", field: "name", value: "x", extra: true }] },
    { kind: "observed", scalar_type: "string", path: observedPath(), value: "guess" },
    { kind: "observed", scalar_type: "string", expected: { kind: "length", path: [field("observations")] } },
    { kind: "observed", scalar_type: "string" },
    { kind: "observed", scalar_type: "boolean", path: observedPath() }, // Program returns string.
  ];
  for (const result of malformed) {
    const invalid = declaration(); invalid.semantics.tasks[0].verification.result = result;
    assert.throws(() => prepareSemanticCatalog(invalid));
  }
});

test("dynamic evidence must exist, match uniquely and agree in exact scalar type and value", () => {
  const prepared = prepareSemanticCatalog(declaration()).prepare("read_current", {});
  const mutations = [
    (r) => { delete r.observations; },
    (r) => { r.observations.rows = []; },
    (r) => { r.observations.rows.push({ ...r.observations.rows[0] }); },
    (r) => { r.observations.rows[0].name = "other"; },
    (r) => { r.observations.rows[0].value = "stale"; },
    (r) => { r.observations.rows[0].value = true; },
    (r) => { r.observations.rows[0].value = null; },
    (r) => { delete r.observations.rows[0].value; },
    (r) => { r.result.value = { value: "current" }; },
    (r) => { r.result.type = "boolean"; },
    (r) => { delete r.result.value; },
    (r) => { r.proof = false; },
    (r) => { r.effects = 2; },
    (r) => { r.fuel_remaining = 101; },
    (r) => { r.profile = "foreign"; },
    (r) => { r.kind = "wrong"; },
  ];
  for (const mutate of mutations) {
    const value = report("string", "current"); mutate(value);
    assert.equal(prepared.verify(value), false);
  }
  assert.equal(prepared.verify(exact(report("string", "current")).replace('"proof":true', '"proof":true,"proof":true')), false);
  const boolean = prepareSemanticCatalog(declaration("boolean")).prepare("read_current", {});
  for (const value of ["true", "false", 1, 0, null, {}]) assert.equal(boolean.verify(report("boolean", value)), false);
  assert.equal(boolean.verify({ ...report("boolean", true), observations: { rows: [{ name: "selected", value: false }] } }), false);
});

test("observed integer equality preserves full u64 precision and rejects numeric coercion", () => {
  const prepared = prepareSemanticCatalog(declaration("integer")).prepare("read_current", {});
  const max = exact(report("integer", 18_446_744_073_709_551_615n));
  assert.equal(prepared.verify(max), true);
  assert.equal(prepared.verify(max.replace('"value":18446744073709551615', '"value":18446744073709551614')), false);
  assert.equal(prepared.verify(JSON.parse(max)), false);
  for (const number of ["-1", "-0", "3.0", "3e0", "18446744073709551616"]) {
    assert.equal(prepared.verify(exact(report("integer", 3n)).replaceAll('"value":3', `"value":${number}`)), false);
  }
  const string = prepareSemanticCatalog(declaration()).prepare("read_current", {});
  assert.equal(string.verify(report("string", "界".repeat(1365) + "x")), true);
  assert.equal(string.verify(report("string", "界".repeat(1366))), false);
  assert.equal(string.verify(report("string", "\ud800")), false);
});

test("dynamic bindings consume the verified actual value from unchanged catalogs across submissions", async () => {
  for (const [kind, values, expected] of [["string", ["first ✓", "second ✓"], ["first ✓", "second ✓"]],
    ["boolean", [true, false], ["true", "false"]], ["integer", [3n, 18_446_744_073_709_551_615n], ["3", "18446744073709551615"]]]) {
    const f = workflow(kind, values);
    const first = await f.session.submit("copy the current observed value");
    const second = await f.session.submit("copy the new observed value");
    assert.equal(first.status, "verified"); assert.equal(second.status, "verified");
    assert.deepEqual(f.writes, expected);
    assert.deepEqual(first.plan, second.plan);
    assert.equal(first.plan.steps[1].arguments.message, "");
    assert.equal(first.steps[1].plan.arguments.message, expected[0]);
    assert.equal(second.steps[1].plan.arguments.message, expected[1]);
  }
  assert.throws(() => prepareSemanticWorkflow(declaration(), steps("decimal")));
});

test("unverified dynamic evidence and application rejection never reach a bound target", async () => {
  for (const options of [
    { read: async (r) => { r.observations.rows[0].value = "changed"; return exact(r); } },
    { read: async (r) => { r.proof = false; return exact(r); } },
    { verify: async () => false },
  ]) {
    const f = workflow("string", ["current"], options);
    const result = await f.session.submit("stop invalid evidence");
    assert.equal(result.status, "failed");
    assert.equal(result.steps[0].status, "failed");
    assert.equal(result.possiblePartialEffects, true);
    assert.equal(f.plans.length, 1);
    assert.deepEqual(f.writes, []);
  }
});

test("cancellation holds a dynamic source verification and prevents late result propagation", async () => {
  const pending = deferred(); let forwarded;
  const f = workflow("string", ["must not save"], { verify: async (_r, _p, signal) => { forwarded = signal; return pending.promise; } });
  const promise = f.session.submit("cancel dynamic read");
  await until(() => forwarded !== undefined);
  f.session.cancel();
  assert.equal(forwarded.aborted, true);
  assert.equal(f.session.busy, true);
  await assert.rejects(f.session.submit("overlap"), /agent_task_pending/u);
  pending.resolve(true);
  const result = await promise;
  assert.equal(result.status, "cancelled");
  assert.equal(result.steps[0].status, "cancelled");
  assert.deepEqual(f.writes, []);
});

test("model planners receive v2 evidence contracts and use references without predicting values", async () => {
  const raw = exact(declaration());
  const context = { taskId: 1, message: "read current text then save that value", catalog: raw };
  const provider = createModelWorkflowProvider({ resultBindings: true, async complete(request) {
    assert.match(request.system, /kind is "observed"/u);
    assert.match(request.system, /verification\.result\.scalar_type/u);
    assert.match(request.system, /Never invent or predict an observed result value/u);
    assert.equal(JSON.parse(request.user).catalog, raw);
    return JSON.stringify({ steps: steps() });
  } });
  const plan = await provider.plan(context, new AbortController().signal);
  assert.deepEqual(plan, prepareSemanticWorkflow(raw, steps()));
  const single = createModelProvider({ async complete() { return '{"task":"read_current","arguments":{}}'; } });
  assert.equal((await single.plan(context, new AbortController().signal)).task, "read_current");
  const defaultWorkflow = createModelWorkflowProvider({ async complete() { return JSON.stringify({ steps: steps() }); } });
  await assert.rejects(defaultWorkflow.plan(context, new AbortController().signal), /invalid_model_response/u);
});
