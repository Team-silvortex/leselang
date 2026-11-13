import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { createSemanticProvider } from "../dist/index.js";

const original = await readFile(new URL("../../../consumer/examples/catalog_agent/fixtures/tauri-semantics-v1.json", import.meta.url), "utf8");
const calculator = await readFile(new URL("../../../consumer/examples/catalog_agent/fixtures/calculator-semantics-v1.json", import.meta.url), "utf8");
const context = (message, catalog = original) => ({ taskId: 1, message, catalog });
const signal = () => new AbortController().signal;

function renamed() {
  const catalog = JSON.parse(original.replaceAll("9223372036854775807", "9007199254740991"));
  const renames = new Map([
    ["tauri.typescript", "custom.notes"], ["tauri_run", "notes_run"], ["fill_and_save", "publish_note"],
    ["gui.list", "notes.list"], ["gui.find", "notes.find"], ["gui.observe", "notes.observe"],
    ["gui.set_text", "notes.replace"], ["gui.click", "notes.publish"],
    ["input", "draft"], ["save", "publish"], ["other", "archive"], ["text", "message"],
  ]);
  function change(value) {
    if (typeof value === "string") return renames.get(value) ?? value;
    if (Array.isArray(value)) return value.map(change);
    if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, (key === "kind" && item === "input") || key === "execution" ? item : change(item)]));
    return value;
  }
  return JSON.stringify(change(catalog));
}

test("the reusable provider discovers renamed tasks, operations, controls and inputs", async () => {
  const provider = createSemanticProvider();
  const text = " quote \" \\ \n新应用 ✓";
  const result = await provider.plan(context(`task publish_note:${text}`, renamed()), signal());
  assert.equal(result.task, "publish_note");
  assert.equal(result.arguments.message, text);
  assert.deepEqual(Object.keys(result.arguments), ["message"]);
  assert.match(result.source, /notes\.replace/);
  assert.match(result.source, /notes\.publish/);
  assert.match(result.source, /"draft"/);
  assert.equal(result.source.includes(text), false);
  await assert.rejects(provider.plan(context("save:content", renamed()), signal()));
  await assert.rejects(provider.plan(context("task fill_and_save:content", renamed()), signal()));
});

test("no-input tasks are selected from the same semantic catalog interface", async () => {
  const provider = createSemanticProvider();
  const result = await provider.plan(context("task calculate_one_plus_two", calculator), signal());
  assert.equal(result.task, "calculate_one_plus_two");
  assert.equal(Object.keys(result.arguments).length, 0);
  assert.match(result.source, /gui\.press/);
  await assert.rejects(provider.plan(context("task calculate_one_plus_two:", calculator), signal()));
  await assert.rejects(provider.plan(context("task fill_and_save", original), signal()));
});

test("aliases are app-owned copied selections and cannot override declared tasks", async () => {
  const aliases = { publish: "publish_note" };
  const provider = createSemanticProvider({ aliases });
  aliases.publish = "another_task";
  assert.equal((await provider.plan(context("publish:text", renamed()), signal())).task, "publish_note");
  await assert.rejects(provider.plan(context("publish:text", original), signal()));
  for (const aliases of [{ "bad alias": "publish_note" }, { alias: "bad.task" }, { "": "publish_note" }]) {
    assert.throws(() => createSemanticProvider({ aliases }));
  }
});

test("provider boundaries preserve exact UTF-8 text and reject malformed input before execution", async () => {
  const provider = createSemanticProvider();
  assert.equal((await provider.plan(context(`task fill_and_save:${"x".repeat(4096)}`), signal())).arguments.text.length, 4096);
  for (const text of ["\0", "\ud800", "\udc00", "x".repeat(4097)]) {
    await assert.rejects(provider.plan(context(`task fill_and_save:${text}`), signal()));
  }
  const controller = new AbortController(); controller.abort();
  await assert.rejects(provider.plan(context("task fill_and_save:text"), controller.signal));
  let coerced = false;
  await assert.rejects(provider.plan(context({ toString() { coerced = true; return "task fill_and_save:text"; } }), signal()));
  assert.equal(coerced, false);
  await assert.rejects(provider.plan(context("x".repeat(8193)), signal()));
});
