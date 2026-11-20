import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { defineRegisteredTask, defineSemanticCatalog, prepareSemanticCatalog } from "../dist/index.js";

const described = JSON.parse(await readFile(new URL("../../../fixtures/host-semantics/tauri-agent-semantics-v2.json", import.meta.url), "utf8"));
const { semantics, ...base } = described;
const literal = (kind, value) => ({ kind: "literal", value: kind === "none" ? { kind } : { kind, value } });
const field = (binding, name) => ({ kind: "field", binding, name });
const path = (name) => [{ kind: "field", name }];
const plain = (value) => JSON.parse(JSON.stringify(value, (_, entry) => typeof entry === "bigint" ? Number(entry) : entry));

function definition(kind = "read_control") {
  return {
    id: "inspect", summary: "Inspect the explicitly selected registered control.", inputs: [], execution: "no_text",
    steps: [{ kind, control: "review", binding: "observed" }],
    failure: { kind: "string", value: "failed" },
    result: field("observed", "value"),
    verification: { kind: "builder_run", max_fuel: 10_000, require_verified: false,
      result: { kind: "observed", scalar_type: "string", path: path("observed_value") },
      predicates: [{ id: "fresh", left: { kind: "scalar", scalar_type: "boolean", path: path("fresh") },
        relation: { kind: "equal" }, right: literal("boolean", true) }],
    },
  };
}
function catalog(task, native = base) {
  return defineSemanticCatalog(native, { version: 2, operations: semantics.operations, tasks: [task] });
}
function prepare(task, arguments_ = {}) { return prepareSemanticCatalog(catalog(task)).prepare(task.id, arguments_); }
function report(effects, value = "done") {
  return { schema_version: 1, kind: "builder_run", profile: base.profile,
    result: { type: "string", value }, effects, fuel_remaining: 9000, observed_value: value, fresh: true };
}
const calls = (task) => task.steps.filter((step) => step.kind === "call");
const guards = (task) => task.steps.filter((step) => step.kind === "guard");

test("a read declaration expands to ordinary find/observe and plans against the actual native catalog", () => {
  const task = defineRegisteredTask(definition());
  assert.deepEqual(calls(task).map((step) => step.operation), ["gui.find", "gui.observe"]);
  assert.equal(guards(task).length, 0, "reading a disabled object does not require actionability");
  assert.equal(calls(task)[1].binding, "observed");
  const prepared = prepare(task);
  assert.equal(prepared.expectedEffects, 2);
  assert.equal(prepared.verify(report(2)), true);
  assert.ok(prepared.plan.source.includes("gui.find"));
  assert.ok(prepared.plan.source.includes("gui.observe"));
  assert.equal(prepared.plan.source.includes("read_control"), false, "there is no macro execution opcode");
});

test("click expansion checks the prior role/enabled state and acknowledgement, then observes without demanding re-enablement", () => {
  const options = definition("click_control"); options.steps[0].before = "prior";
  const task = defineRegisteredTask(options);
  assert.deepEqual(calls(task).map((step) => step.operation), ["gui.find", "gui.observe", "gui.click", "gui.observe"]);
  assert.equal(calls(task)[1].binding, "prior");
  assert.equal(calls(task)[3].binding, "observed");
  assert.deepEqual(plain(guards(task)[0].conditions), [
    { kind: "equal", left: field("prior", "role"), right: literal("string", "button") },
    { kind: "equal", left: field("prior", "enabled"), right: literal("boolean", true) },
  ]);
  assert.deepEqual(plain(calls(task)[2].arguments[1]), { name: "expected_revision", value: field("prior", "revision") });
  assert.equal(guards(task).length, 2, "the builder does not invent a business count/status postcondition");
  assert.equal(task.steps.at(-1).binding, "observed", "the final step is a fresh observation, even when the action disabled itself");
  assert.equal(prepare(task).expectedEffects, 4);
});

test("text expansion checks editability before submission and the exact observed value afterwards", () => {
  const options = definition("set_control_text");
  options.inputs = [{ name: "message", scalar_type: "string" }]; options.execution = "named_text";
  options.steps[0] = { kind: "set_control_text", control: "draft-input", binding: "observed", before: "prior",
    text: { kind: "input", name: "message" } };
  const task = defineRegisteredTask(options);
  assert.equal(guards(task)[0].conditions.length, 3);
  assert.deepEqual(plain(guards(task)[0].conditions[2]),
    { kind: "equal", left: field("prior", "editable"), right: literal("boolean", true) });
  assert.deepEqual(plain(guards(task).at(-1).conditions), [
    { kind: "equal", left: field("observed", "role"), right: literal("string", "text_field") },
    { kind: "equal", left: field("observed", "value"), right: { kind: "input", name: "message" } },
  ]);
  const text = 'literal "data" \\ path\n编辑 ✓';
  const prepared = prepare(task, { message: text });
  assert.equal(prepared.expectedEffects, 4);
  assert.equal(prepared.plan.source.includes(text), false);
  assert.equal(prepared.plan.arguments.message, text);
  assert.equal(prepared.verify(report(4, text)), true);
});

test("raw semantic calls and guards compose with control steps and explicit before snapshots", () => {
  const options = definition("click_control");
  options.inputs = [{ name: "text", scalar_type: "string" }]; options.execution = "text";
  options.steps = [
    { kind: "call", binding: "ids", operation: "gui.list", arguments: [] },
    { kind: "guard", conditions: [{ kind: "contains", list: { kind: "binding", name: "ids" }, value: literal("string", "review") }],
      failure: options.failure },
    { kind: "set_control_text", control: "draft", binding: "draft_after", text: { kind: "input", name: "text" } },
    { kind: "click_control", control: "review", binding: "observed", before: "prior" },
    { kind: "guard", conditions: [{ kind: "increased_by", before: field("prior", "clicks"), after: field("observed", "clicks"), amount: 1 }],
      failure: options.failure },
  ];
  const task = defineRegisteredTask(options), prepared = prepare(task, { text: "draft" });
  assert.equal(prepared.expectedEffects, 9);
  assert.equal(task.steps.length, 16);
  assert.ok(prepared.plan.source.includes("_lese_contains"));
  assert.ok(prepared.plan.source.includes('field(value: prior, name: "clicks")'));
});

test("business verification is explicit and preserved rather than inferred from an acknowledgement", () => {
  const options = definition("click_control"), task = defineRegisteredTask(options);
  assert.deepEqual(plain(task.verification), options.verification);
  const prepared = prepare(task), actual = report(4);
  assert.equal(prepared.verify(actual), true);
  actual.fresh = false;
  assert.equal(prepared.verify(actual), false);
  actual.fresh = true; actual.observed_value = "not the returned value";
  assert.equal(prepared.verify(actual), false);
  delete options.verification;
  assert.throws(() => defineRegisteredTask(options), /invalid_semantics/);
});

test("builder declarations own and deeply freeze all caller data without altering it", () => {
  const options = definition("click_control"), before = structuredClone(options);
  const task = defineRegisteredTask(options);
  assert.deepEqual(options, before);
  const source = prepare(task).plan.source;
  options.steps[0].control = "different";
  options.failure.value = "changed";
  options.verification.predicates[0].right.value.value = false;
  assert.equal(prepare(task).plan.source, source);
  assert.equal(task.verification.predicates[0].right.value.value, true);
  assert.throws(() => task.steps.push({}), TypeError);
  assert.throws(() => { task.steps[0].arguments[0].value.value.value = "changed"; }, TypeError);
  assert.throws(() => { task.verification.result.path[0].name = "result"; }, TypeError);
});

test("authoring refuses getters, toJSON, cycles, symbols, holes and unbounded declarations without running hooks", () => {
  let touched = 0;
  const accessor = definition();
  Object.defineProperty(accessor.steps[0], "control", { enumerable: true, get() { touched++; return "review"; } });
  const serialize = definition(); serialize.toJSON = () => { touched++; return definition(); };
  const cycle = definition(); cycle.result = cycle;
  const hole = definition(); hole.steps.length = 2;
  const symbol = definition(); symbol[Symbol("extra")] = true;
  const proto = Object.assign(Object.create({ inherited: true }), definition());
  const huge = definition(); huge.verification.predicates[0].right = literal("string", "x".repeat(256 * 1024));
  for (const candidate of [accessor, serialize, cycle, hole, symbol, proto, huge]) {
    assert.throws(() => defineRegisteredTask(candidate), /invalid_semantics/);
  }
  assert.equal(touched, 0);
});

test("generated names avoid supplied bindings, inputs and even undeclared or forward references", () => {
  const options = definition();
  options.steps[0].binding = "_registered_0";
  options.result = field("_registered_0", "value");
  const task = defineRegisteredTask(options);
  assert.notEqual(calls(task)[0].binding, "_registered_0");
  assert.equal(prepare(task).expectedEffects, 2);

  for (const reference of ["_registered_0", "_registered_1", "_registered_2"]) {
    const invalid = definition("click_control");
    invalid.result = field(reference, "value");
    const expanded = defineRegisteredTask(invalid);
    assert.equal(calls(expanded).some((call) => call.binding === reference), false);
    assert.throws(() => catalog(expanded), /field requires native binding/);
  }
  const forward = definition();
  forward.steps.unshift({ kind: "guard", conditions: [{ kind: "equal", left: field("later", "value"), right: literal("string", "pending") }], failure: forward.failure });
  forward.steps.push({ kind: "read_control", control: "archive", binding: "later" });
  assert.throws(() => catalog(defineRegisteredTask(forward)), /field requires native binding/);
});

test("generated names cannot capture undeclared raw-call operands or text expressions", () => {
  const input = definition();
  input.steps.push({ kind: "call", binding: "other", operation: "gui.observe",
    arguments: [{ name: "target", value: { kind: "binding", name: "_registered_0" } }] });
  const expanded = defineRegisteredTask(input);
  assert.equal(calls(expanded).some((call) => call.binding === "_registered_0"), false);
  assert.throws(() => catalog(expanded), /unknown or forward binding/);

  const text = definition("set_control_text");
  text.steps[0].text = field("_registered_1", "value");
  assert.throws(() => catalog(defineRegisteredTask(text)), /field requires native binding/);
});

test("macro and option admission refuses malformed identifiers, fields, input conventions and duplicate public names", () => {
  const invalid = [];
  for (const value of ["", "with spaces", "x".repeat(65), "fn", "body", "_lese_hidden", "1name"]) {
    const option = definition(); option.steps[0].binding = value; invalid.push(option);
  }
  for (const value of ["", "with spaces", "x".repeat(65), "\ud800", "path/name"]) {
    const option = definition(); option.steps[0].control = value; invalid.push(option);
  }
  for (const change of [
    (o) => { o.extra = true; },
    (o) => { o.steps[0].extra = true; },
    (o) => { o.steps[0].kind = "arbitrary_action"; },
    (o) => { o.steps[0].before = "prior"; },
    (o) => { o.steps[0] = { kind: "click_control", control: "review", binding: "same", before: "same" }; },
    (o) => { o.steps.push({ kind: "read_control", control: "archive", binding: "observed" }); },
    (o) => { o.inputs = [{ name: "observed", scalar_type: "string" }]; o.execution = "text"; },
    (o) => { o.inputs = [{ name: "message", scalar_type: "boolean" }]; o.execution = "text"; },
    (o) => { o.inputs = [{ name: "first", scalar_type: "string" }, { name: "second", scalar_type: "string" }]; o.execution = "text"; },
    (o) => { o.execution = "text"; },
    (o) => { o.failure = { kind: "boolean", value: false }; },
    (o) => { o.steps = []; },
    (o) => { o.steps = [{ kind: "set_control_text", control: "draft", binding: "observed" }]; },
  ]) { const option = definition(); change(option); invalid.push(option); }
  for (const candidate of invalid) assert.throws(() => defineRegisteredTask(candidate), /invalid_semantics/);
});

test("expanded step and call budgets are enforced without truncating an application program", () => {
  const read = definition();
  read.steps = Array.from({ length: 8 }, (_, i) => ({ kind: "read_control", control: "review", binding: `read_${i}` }));
  read.result = field("read_7", "value");
  assert.equal(prepare(defineRegisteredTask(read)).expectedEffects, 16);
  read.steps.push({ kind: "read_control", control: "review", binding: "read_8" });
  assert.throws(() => defineRegisteredTask(read), /step or call bound/);

  const text = definition();
  text.steps = Array.from({ length: 3 }, (_, i) => ({ kind: "set_control_text", control: "draft", binding: `write_${i}`, text: literal("string", "value") }));
  text.result = field("write_2", "value");
  assert.equal(defineRegisteredTask(text).steps.length, 21);
  text.steps.push({ kind: "read_control", control: "draft", binding: "extra" });
  assert.throws(() => defineRegisteredTask(text), /step or call bound/);
});

test("exact authored u64 values survive expansion, while unsafe Number values remain rejected", () => {
  const maximum = 18_446_744_073_709_551_615n;
  const options = definition();
  options.failure = { kind: "integer", value: maximum };
  options.result = literal("integer", maximum);
  options.verification.result = { kind: "integer", value: maximum };
  const task = defineRegisteredTask(options), raw = catalog(task);
  assert.equal(task.result.value.value, maximum);
  assert.ok(raw.includes('"value":18446744073709551615'));
  assert.ok(prepare(task).plan.source.includes("18446744073709551615"));
  for (const value of [Number(maximum), maximum + 1n, -1n]) {
    options.result.value.value = value;
    assert.throws(() => defineRegisteredTask(options), /invalid_semantics/);
  }
});

test("actual catalog types, declared operations and report predicates remain authoritative after expansion", () => {
  const task = defineRegisteredTask(definition("click_control"));
  const missing = structuredClone(base);
  missing.operations = missing.operations.filter((operation) => operation.name !== "gui.click");
  assert.throws(() => catalog(task, missing), /semantic operation reference/);
  const wrong = structuredClone(base);
  wrong.operations.find((operation) => operation.name === "gui.click").result = { kind: "scalar", name: "string" };
  assert.throws(() => catalog(task, wrong), /condition type/);
  const option = definition();
  option.verification.predicates[0].right = literal("string", "true");
  assert.throws(() => catalog(defineRegisteredTask(option)), /predicate type mismatch/);
});
