import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { createSemanticProvider, prepareSemanticCatalog } from "@leselang/bridge";

const fixture = (name) => readFileSync(new URL(`../../../consumer/examples/catalog_agent/fixtures/${name}`, import.meta.url), "utf8");
const raw = fixture("tauri-agent-semantics-v1.json");
const text = 'message "引号" \\ 路径\nsecond line ✓';

function controls() {
  return [
    { identifier: "save", role: "button", title: "Save", value: "old", editable: false, enabled: true, clicks: 2 },
    { identifier: "other", role: "button", title: "Other control", value: "", editable: false, enabled: true, clicks: 0 },
    { identifier: "input", role: "text_field", title: "Text to save", value: "old", editable: true, enabled: true, clicks: 0 },
  ];
}

function report(type, value, write = false) {
  const before = controls(), after = structuredClone(before);
  if (write) {
    after[0].value = after[2].value = text;
    after[0].clicks += 1;
  }
  return { schema_version: 1, kind: "tauri_task_run", profile: "tauri.typescript",
    result: { type, value }, effects: write ? 9 : 3, fuel_remaining: 9000,
    native_before: before, native_after: after };
}

test("the interactive catalog generates Rust-executed fixtures for each supported task shape", () => {
  const catalog = prepareSemanticCatalog(raw);
  assert.deepEqual(catalog.tasks.map(({ id, inputName }) => [id, inputName]), [
    ["fill_and_save", "text"], ["save_message", "message"], ["inspect_controls", undefined], ["check_controls", undefined],
  ]);
  for (const [task, args, source, type, value, write] of [
    ["fill_and_save", { text }, "form-semantics.lese", "boolean", true, true],
    ["save_message", { message: text }, "tauri-save-message.lese", "string", "saved", true],
    ["inspect_controls", {}, "tauri-inspect-controls.lese", "integer", 3, false],
    ["check_controls", {}, "tauri-check-controls.lese", "none", null, false],
  ]) {
    const prepared = catalog.prepare(task, args);
    assert.equal(prepared.plan.source, fixture(source));
    assert.equal(prepared.expectedEffects, write ? 9 : 3);
    assert.deepEqual({ ...prepared.plan.arguments }, args);
    assert.ok(!prepared.plan.source.includes(text), "input data cannot become source");
    const evidence = report(type, value, write);
    assert.equal(Object.hasOwn(evidence, "verified"), false);
    assert.equal(prepared.verify(evidence), true, task);
    evidence.native_after.reverse();
    assert.equal(prepared.verify(JSON.stringify(evidence)), true, task);
    assert.equal(prepared.verify({ ...evidence, result: { type: "boolean", value: false } }), false, task);
    assert.equal(prepared.verify({ ...evidence, kind: "tauri_run", verified: true }), false, task);
    assert.equal(prepared.verify({ ...evidence, result: { type } }), false, task);
  }
});

test("none and integer read-only outcomes still require all observation and guard postconditions", () => {
  const catalog = prepareSemanticCatalog(raw);
  for (const [task, type, value] of [["inspect_controls", "integer", 3], ["check_controls", "none", null]]) {
    const prepared = catalog.prepare(task, {});
    const changes = [
      (r) => { r.native_before[0].role = r.native_after[0].role = "text_field"; },
      (r) => { r.native_before[0].enabled = r.native_after[0].enabled = false; },
      (r) => { r.native_before[1].identifier = r.native_after[1].identifier = "missing_other"; },
      (r) => { r.native_after[0].value = "changed"; },
      (r) => { r.native_after[1].clicks += 1; },
      (r) => { r.native_before.pop(); r.native_after.pop(); },
      (r) => { r.native_before[1] = r.native_before[0]; r.native_after[1] = r.native_after[0]; },
      (r) => { r.effects = 1; },
    ];
    for (const change of changes) {
      const evidence = report(type, value); change(evidence);
      assert.equal(prepared.verify(evidence), false, `${task}: ${change}`);
    }
    assert.throws(() => catalog.prepare(task, { text: "unexpected" }));
  }
  const string = catalog.prepare("save_message", { message: text });
  assert.equal(string.verify(report("string", "failed", true)), false);
  const unsaved = report("string", "saved", true); unsaved.native_after[0].value = "different";
  assert.equal(string.verify(unsaved), false);
  assert.throws(() => catalog.prepare("save_message", { text }));
  assert.throws(() => catalog.prepare("save_message", {}));
});

test("one public provider selects zero-input, renamed-input and legacy commands from the same catalog", async () => {
  const provider = createSemanticProvider({ aliases: { 保存: "fill_and_save" } });
  const signal = new AbortController().signal;
  const catalog = prepareSemanticCatalog(raw);
  for (const [message, task, args] of [
    [`保存：${text}`, "fill_and_save", { text }],
    [`task save_message:${text}`, "save_message", { message: text }],
    ["task inspect_controls", "inspect_controls", {}],
    ["任务 check_controls", "check_controls", {}],
  ]) {
    const plan = await provider.plan({ taskId: 1, message, catalog: raw }, signal);
    assert.deepEqual(plan, catalog.prepare(task, args).plan);
  }
  for (const message of ["task inspect_controls:", "task check_controls:unexpected", "task save_message", "task undeclared"]) {
    await assert.rejects(provider.plan({ taskId: 2, message, catalog: raw }, signal));
  }
});
