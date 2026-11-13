import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { prepareSemanticCatalog } from "@leselang/bridge";

const fixture = (name) => readFileSync(new URL(`../../../consumer/examples/catalog_agent/fixtures/${name}`, import.meta.url), "utf8");
const form = fixture("tauri-semantics-v1.json");
const calculator = fixture("calculator-semantics-v1.json");
const safeForm = () => JSON.parse(form.replaceAll("9223372036854775807", "4294967295"));
const encode = (value) => JSON.stringify(value);
const text = 'quote " slash \\ newline\n保存 ✓';
function formReport(value = text) {
  const before = [
    { identifier: "save", role: "button", title: "Save", value: "old", editable: false, enabled: true, clicks: 2 },
    { identifier: "other", role: "button", title: "Other control", value: "", editable: false, enabled: true, clicks: 0 },
    { identifier: "input", role: "text_field", title: "Text to save", value: "old", editable: true, enabled: true, clicks: 0 },
  ];
  const after = structuredClone(before);
  after[0].value = value; after[0].clicks = 3; after[2].value = value;
  return { schema_version: 1, kind: "tauri_run", profile: "tauri.typescript",
    result: { type: "boolean", value: true }, effects: 9, fuel_remaining: 9000, verified: true,
    native_before: before, native_after: after };
}
function calculatorReport() {
  const before = { identifier: "calculator", role: "AXWindow", display: "0", last_expression: null,
    buttons: ["one", "add", "two", "equals"].map((name) => ({ name, enabled: true })) };
  return { schema_version: 1, kind: "macos_ax_calculator_run", profile: "macos_ax.calculator",
    result: { type: "string", value: "3" }, effects: 10, fuel_remaining: 1600, verified: true,
    native_before: [before], native_after: [{ ...structuredClone(before), display: "3", last_expression: "1 + 2" }] };
}
function scalarCatalog() {
  return {
    schema_version: 1, protocol_version: 1, catalog_version: 8, profile: "thirdparty.counter",
    operations: [{ name: "counter.read", required_capability: "counter.read", parameters: [], result: { kind: "scalar", name: "integer" } }],
    native_types: [],
    semantics: { version: 1,
      operations: [{ operation: "counter.read", summary: "Read the scoped counter.", effect: "read", completion: "value" }],
      tasks: [{ id: "inspect_counter", summary: "Inspect and verify a counter.", inputs: [],
        steps: [{ kind: "call", binding: "count", operation: "counter.read", arguments: [] }],
        result: { kind: "binding", name: "count" }, execution: "no_text",
        verification: { kind: "counter_run", max_fuel: 100, result: { kind: "integer", value: 42 }, require_verified: false,
          predicates: [{ id: "counter_value", left: { kind: "scalar", path: [{ kind: "field", name: "count" }], scalar_type: "integer" },
            relation: { kind: "equal" }, right: { kind: "literal", value: { kind: "integer", value: 42 } } }] } }],
    },
  };
}
function scalarReport(value = "42") {
  return `{"schema_version":1,"kind":"counter_run","profile":"thirdparty.counter","result":{"type":"integer","value":${value}},"effects":1,"fuel_remaining":50,"count":${value}}`;
}

test("three real metadata catalogs generate the exact Rust compiler-checked source", () => {
  for (const profile of ["appkit", "tauri", "calculator"]) {
    const raw = fixture(`${profile}-semantics-v1.json`);
    const catalog = prepareSemanticCatalog(raw);
    const task = profile === "calculator" ? "calculate_one_plus_two" : "fill_and_save";
    const prepared = catalog.prepare(task, profile === "calculator" ? {} : { text });
    assert.equal(prepared.plan.source, fixture(profile === "calculator" ? "calculator-semantics.lese" : "form-semantics.lese"));
    assert.equal(prepared.plan.task, task);
    assert.equal(prepared.expectedEffects, profile === "calculator" ? 10 : 9);
    assert.ok(Object.isFrozen(catalog));
    assert.ok(Object.isFrozen(catalog.tasks));
    assert.ok(Object.isFrozen(catalog.tasks[0]));
    assert.ok(Object.isFrozen(prepared));
    assert.ok(Object.isFrozen(prepared.plan));
    assert.ok(Object.isFrozen(prepared.plan.arguments));
    assert.ok(Object.isFrozen(prepared.checks));
    assert.equal(prepared.plan.source.includes(text), false);
  }
  const planned = prepareSemanticCatalog(form).prepare("fill_and_save", { text });
  assert.equal(planned.verify(formReport()), true);
  const reordered = formReport(); reordered.native_after.reverse();
  assert.equal(planned.verify(reordered), true);
  assert.equal(planned.verify(encode(reordered)), true);
  assert.equal(prepareSemanticCatalog(calculator).prepare("calculate_one_plus_two", {}).verify(calculatorReport()), true);
});

test("operation, control, input, task and profile names come only from the catalog", () => {
  const value = safeForm();
  const rename = new Map([
    ["tauri.typescript", "thirdparty.notes"], ["tauri_run", "notes_run"], ["fill_and_save", "publish_note"],
    ["gui.list", "notes.enumerate"], ["gui.find", "notes.find"], ["gui.observe", "notes.read"],
    ["gui.set_text", "notes.replace"], ["gui.click", "notes.publish"], ["input", "draft"],
    ["save", "publish"], ["other", "archive"], ["text", "message"],
  ]);
  const visit = (value, key) => {
    if (typeof value === "string") return key === "execution" || key === "kind" && value === "input" ? value : rename.get(value) ?? value;
    if (Array.isArray(value)) return value.map((entry) => visit(entry));
    if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, value]) => [key, visit(value, key)]));
    return value;
  };
  const changed = visit(value);
  const catalog = prepareSemanticCatalog(changed);
  assert.deepEqual(catalog.tasks, [{ id: "publish_note", summary: value.semantics.tasks[0].summary, inputName: "message" }]);
  const prepared = catalog.prepare("publish_note", { message: text });
  assert.match(prepared.plan.source, /notes\.replace/u);
  assert.match(prepared.plan.source, /name: "draft"/u);
  assert.doesNotMatch(prepared.plan.source, /gui\./u);
  assert.equal(prepared.verify(visit(formReport())), true);
  assert.throws(() => catalog.prepare("fill_and_save", { message: text }));
  assert.throws(() => catalog.prepare("publish_note", { text }));
  assert.throws(() => catalog.prepare("publish_note", {}));
  assert.throws(() => catalog.prepare("publish_note", { message: text, extra: "unused" }));
  assert.throws(() => catalog.prepare("publish_note", { message: "x".repeat(4097) }));
});

test("all unused semantic tasks and their actual typed references are validated", () => {
  const changes = [
    (c) => { delete c.semantics; },
    (c) => { c.semantics = null; },
    (c) => { c.semantics.version = 2; },
    (c) => { c.semantics.unrecognized = true; },
    (c) => { c.semantics.tasks[0].unknown = true; },
    (c) => { c.semantics.tasks.push(structuredClone(c.semantics.tasks[0])); },
    (c) => { c.semantics.operations[0].operation = "not.registered"; },
    (c) => { c.semantics.operations[0].completion = "automatically_verified"; },
    (c) => { c.semantics.operations[0].effect = "safe"; },
    (c) => { c.semantics.operations = []; },
    (c) => { c.semantics.tasks[0].steps[0].arguments = [{ name: "unknown", value: { kind: "literal", value: { kind: "string", value: "x" } } }]; },
    (c) => { c.semantics.tasks[0].steps[0].binding = "text"; },
    (c) => { c.semantics.tasks[0].steps[2].arguments[0].value = { kind: "binding", name: "input_after" }; },
    (c) => { c.semantics.tasks[0].steps[4].conditions[0].left.name = "undeclared"; },
    (c) => { c.semantics.tasks[0].steps[4].conditions[0].right.value.kind = "integer"; },
    (c) => { c.semantics.tasks[0].verification.predicates[0].relation.typo = true; },
    (c) => { c.semantics.tasks[0].verification.predicates[0].left.path = []; },
    (c) => { c.semantics.tasks[0].verification.predicates = []; },
    (c) => { c.semantics.tasks[0].verification.max_fuel = 1_000_001; },
    (c) => { c.native_types[1].fields.push(c.native_types[1].fields[0]); },
    (c) => { c.operations.push(c.operations[0]); },
  ];
  for (const change of changes) {
    const value = safeForm(); change(value);
    assert.throws(() => prepareSemanticCatalog(value), change.toString());
  }
  const wrongUnused = safeForm();
  const extra = structuredClone(wrongUnused.semantics.tasks[0]); extra.id = "unused_bad";
  extra.steps[0].operation = "missing"; wrongUnused.semantics.tasks.push(extra);
  assert.throws(() => prepareSemanticCatalog(wrongUnused));
  const collision = scalarCatalog(); collision.operations[0].name = "main";
  collision.semantics.operations[0].operation = "main"; collision.semantics.tasks[0].steps[0].operation = "main";
  assert.throws(() => prepareSemanticCatalog(collision).prepare("inspect_counter", {}));
});

test("report headers, complete evidence, typed paths and predicates all remain mandatory", () => {
  const prepared = prepareSemanticCatalog(form).prepare("fill_and_save", { text });
  const changes = [
    (r) => { r.schema_version = 2; }, (r) => { r.profile = "different.app"; }, (r) => { r.kind = "different_run"; },
    (r) => { r.effects = 8; }, (r) => { r.fuel_remaining = 10001; }, (r) => { r.verified = false; },
    (r) => { r.result.type = "string"; }, (r) => { r.result.value = "true"; },
    (r) => { r.native_after[0].clicks = 4; }, (r) => { r.native_after[2].value = "different"; },
    (r) => { r.native_after[1].enabled = false; }, (r) => { r.native_after.push(r.native_after[0]); },
    (r) => { r.native_after[2].identifier = "save"; }, (r) => { delete r.native_before; },
    (r) => { r.native_after[0].title = "x".repeat(4097); }, (r) => { r.native_before[0].clicks = -1; },
    (r) => { r.native_before[2].enabled = "true"; }, (r) => { delete r.native_after[2].editable; },
  ];
  for (const change of changes) { const value = formReport(); change(value); assert.equal(prepared.verify(value), false, change.toString()); }
  assert.equal(prepared.verify(encode(formReport()).replace('"effects":9', '"effects":9,"effects":9')), false);
  const calc = prepareSemanticCatalog(calculator).prepare("calculate_one_plus_two", {});
  const failures = [
    (r) => { delete r.native_before[0].last_expression; },
    (r) => { r.native_before[0].last_expression = 42; },
    (r) => { r.native_after[0].last_expression = "0 + 3"; },
    (r) => { r.native_before[0].display = "15"; },
    (r) => { r.native_after[0].buttons[0].name = "two"; },
    (r) => { r.native_after[0].buttons[0].enabled = false; },
  ];
  for (const change of failures) { const value = calculatorReport(); change(value); assert.equal(calc.verify(value), false); }
  const reordered = calculatorReport(); reordered.native_after[0].buttons.reverse();
  assert.equal(calc.verify(reordered), true);
});

test("raw JSON preserves full u64 integers and never accepts a rounded object Number", () => {
  assert.throws(() => prepareSemanticCatalog(JSON.parse(form)), /unsafe JSON number/u);
  assert.equal(prepareSemanticCatalog(form).prepare("fill_and_save", { text }).verify(formReport()), true);
  for (const token of ["0", "9007199254740991", "9007199254740992", "9223372036854775807", "18446744073709551615"]) {
    const raw = encode(scalarCatalog()).replaceAll('"value":42', `"value":${token}`);
    const prepared = prepareSemanticCatalog(raw).prepare("inspect_counter", {});
    assert.equal(prepared.verify(scalarReport(token)), true, token);
    if (BigInt(token) > BigInt(Number.MAX_SAFE_INTEGER)) assert.equal(prepared.verify(JSON.parse(scalarReport(token))), false);
  }
  for (const token of ["-0", "-1", "1.0", "1e0", "18446744073709551616"]) {
    assert.throws(() => prepareSemanticCatalog(encode(scalarCatalog()).replaceAll('"value":42', `"value":${token}`)), token);
  }
  const scalar = prepareSemanticCatalog(scalarCatalog()).prepare("inspect_counter", {});
  for (const field of ["schema_version", "effects", "fuel_remaining"]) {
    for (const token of ["-0", "1.0", "1e0"]) {
      assert.equal(scalar.verify(scalarReport().replace(new RegExp(`"${field}":[0-9]+`, "u"), `"${field}":${token}`)), false);
    }
  }
  const exactSource = scalarCatalog();
  exactSource.semantics.tasks[0].result = { kind: "literal", value: { kind: "integer", value: 42 } };
  const exact = prepareSemanticCatalog(encode(exactSource).replaceAll('"value":42', '"value":18446744073709551615')).prepare("inspect_counter", {});
  assert.match(exact.plan.source, /body: 18446744073709551615/u);
  assert.equal(exact.verify(scalarReport("18446744073709551615")), true);
  const overflow = scalarCatalog();
  overflow.semantics.tasks[0].verification.predicates[0].relation = { kind: "increased_by", amount: 1 };
  const raw = encode(overflow).replaceAll('"value":42', '"value":18446744073709551615');
  assert.equal(prepareSemanticCatalog(raw).prepare("inspect_counter", {}).verify(scalarReport("18446744073709551615")), false);
  const bounds = formReport();
  bounds.native_before[2].clicks = bounds.native_after[2].clicks = 314159;
  const rawReport = encode(bounds).replaceAll("314159", "9223372036854775807");
  assert.equal(prepareSemanticCatalog(form).prepare("fill_and_save", { text }).verify(rawReport), true);
  assert.equal(prepareSemanticCatalog(form).prepare("fill_and_save", { text }).verify(rawReport.replaceAll("9223372036854775807", "9223372036854775808")), false);
});

test("metadata collections and generated helper/source ceilings stay finite", () => {
  const changes = [
    (c) => { c.semantics.tasks[0].summary = "x".repeat(1025); },
    (c) => { c.semantics.tasks = Array.from({ length: 9 }, (_, i) => ({ ...structuredClone(c.semantics.tasks[0]), id: `task_${i}` })); },
    (c) => { c.semantics.tasks[0].steps = Array.from({ length: 22 }, () => structuredClone(c.semantics.tasks[0].steps[0])); },
    (c) => { c.semantics.tasks[0].inputs.push({ name: "second", scalar_type: "string" }); },
    (c) => { c.semantics.tasks[0].verification.predicates = Array.from({ length: 65 }, (_, i) => ({ ...structuredClone(c.semantics.tasks[0].verification.predicates[0]), id: `check_${i}` })); },
    (c) => { c.semantics.tasks[0].verification.predicates[0].left.path = Array.from({ length: 9 }, () => ({ kind: "field", name: "x" })); },
  ];
  for (const change of changes) { const c = scalarCatalog(); change(c); assert.throws(() => prepareSemanticCatalog(c), change.toString()); }
  const wide = scalarCatalog(), task = wide.semantics.tasks[0];
  task.steps = Array.from({ length: 9 }, (_, i) => ({ kind: "call", binding: `count_${i}`, operation: "counter.read", arguments: [] }));
  task.steps.push({ kind: "guard", conditions: Array.from({ length: 8 }, (_, i) => ({ kind: "equal", left: { kind: "binding", name: `count_${i}` }, right: { kind: "binding", name: "count_8" } })), failure: { kind: "integer", value: 0 } });
  task.result = { kind: "binding", name: "count_8" };
  assert.throws(() => prepareSemanticCatalog(wide).prepare("inspect_counter", {}), /helper parameter bound/u);
  const huge = scalarCatalog(); huge.extra = Array.from({ length: 20 }, () => "x".repeat(16_384));
  assert.throws(() => prepareSemanticCatalog(huge), /JSON byte bound/u);
});

test("JSON ownership, byte bounds, duplicate keys and accessors fail before a proposal", () => {
  assert.throws(() => prepareSemanticCatalog(form.replace('"version":1', '"version":1,"version":1')));
  assert.throws(() => prepareSemanticCatalog(form + "{}"));
  assert.throws(() => prepareSemanticCatalog(" ".repeat(256 * 1024) + form));
  assert.throws(() => prepareSemanticCatalog('{"x":"\\ud800"}'));
  let reads = 0;
  const accessor = safeForm(); Object.defineProperty(accessor, "semantics", { get() { reads += 1; return {}; }, enumerable: true });
  assert.throws(() => prepareSemanticCatalog(accessor)); assert.equal(reads, 0);
  const cycle = safeForm(); cycle.loop = cycle; assert.throws(() => prepareSemanticCatalog(cycle));
  const array = safeForm(); array.operations.extra = true; assert.throws(() => prepareSemanticCatalog(array));
  const sparse = safeForm(); delete sparse.operations[0]; assert.throws(() => prepareSemanticCatalog(sparse));
  const input = safeForm(), catalog = prepareSemanticCatalog(input);
  input.semantics.tasks[0].steps[0].operation = "forged";
  const args = { text }, prepared = catalog.prepare("fill_and_save", args); args.text = "mutated";
  assert.equal(prepared.plan.arguments.text, text); assert.equal(prepared.verify(formReport()), true);
  const unsafe = safeForm(); unsafe.unused = Number.MAX_SAFE_INTEGER + 1;
  assert.throws(() => prepareSemanticCatalog(unsafe));
  const none = safeForm(); delete none.semantics;
  assert.throws(() => prepareSemanticCatalog(none), "no legacy-profile fallback");
});
