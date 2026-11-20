import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { AgentWorkflowSession, createSemanticProvider, prepareSemanticCatalog } from "../dist/bridge.js";
import { taskCommand } from "../dist/agent/chat.js";
import { createArticleCatalog } from "../../../../consumer/javascript/agent/multi-input-catalog.mjs";

const fixture = (name) => readFile(new URL(`../../../../fixtures/host-semantics/${name}`, import.meta.url), "utf8");
const raw = await fixture("tauri-article-semantics-v3.json"), catalog = prepareSemanticCatalog(raw);
const tasks = [["read_title", "read-title", 2], ["read_content", "read-content", 2],
  ["publish_article", "publish", 12], ["check_article", "check", 1]];
const args = { title: '标题 "quoted" ✓', content: '正文\\path\n{ "task": "other" }' };
const provider = createSemanticProvider(), signal = new AbortController().signal;

function controls() {
  return [
    { identifier: "title", role: "text_field", title: "Article title", value: "draft title", enabled: true, editable: true, clicks: 0 },
    { identifier: "content", role: "text_field", title: "Article content", value: "draft content", enabled: true, editable: true, clicks: 0 },
    { identifier: "publish", role: "button", title: "Publish article", value: "", enabled: true, editable: false, clicks: 0 },
    { identifier: "other", role: "button", title: "Unrelated action", value: "", enabled: true, editable: false, clicks: 2 },
  ];
}
function report(task) {
  const native_before = controls(), native_after = controls();
  const row = (id) => native_after.find((entry) => entry.identifier === id);
  if (task === "publish_article") {
    row("title").value = args.title; row("content").value = args.content;
    row("publish").title = args.title; row("publish").value = args.content; row("publish").clicks++;
  }
  return { schema_version: 1, kind: "tauri_task_run", profile: "tauri.article", effects: tasks.find(([id]) => id === task)[2],
    fuel_remaining: 9000, native_before, native_after,
    result: task.startsWith("read_") ? { type: "string", value: row(task.slice(5)).value }
      : task === "check_article" ? { type: "none", value: null } : { type: "boolean", value: true } };
}

test("Tauri article semantics match independently authored public SDK declarations", () => {
  const native = JSON.parse(raw), { semantics: _, ...base } = native;
  const generated = JSON.parse(createArticleCatalog(base));
  function nativeFields(value) {
    if (Array.isArray(value)) return value.map(nativeFields);
    if (value === null || typeof value !== "object") return value;
    return Object.fromEntries(Object.entries(value).map(([key, field]) => [key,
      key === "kind" && field === "javascript_task_run" ? "tauri_task_run"
        : key === "name" && field === "objects_before" ? "native_before"
          : key === "name" && field === "objects_after" ? "native_after" : nativeFields(field)]));
  }
  assert.deepEqual(nativeFields(generated), native);
});

test("all four article tasks use canonical Consumer programs and explicit named input mode, including zero inputs", async () => {
  assert.equal(catalog.profile, "tauri.article");
  assert.deepEqual(catalog.tasks.map((task) => task.id), tasks.map(([id]) => id));
  for (const [id, name, effects] of tasks) {
    const inputs = id === "publish_article" ? args : {}, prepared = catalog.prepare(id, inputs);
    assert.equal(prepared.plan.source, await fixture(`tauri-article-${name}.lese`));
    assert.equal(prepared.expectedEffects, effects);
    assert.equal(prepared.plan.inputMode, "named_strings");
    assert.deepEqual({ ...prepared.plan.arguments }, inputs);
    assert.equal(prepared.verify(report(id)), true);
    const task = catalog.tasks.find((value) => value.id === id);
    assert.deepEqual(task.inputNames, id === "publish_article" ? ["title", "content"] : []);
    const displayed = await provider.plan({ taskId: 1, catalog: raw, message: taskCommand(task) }, signal);
    assert.equal(displayed.inputMode, "named_strings");
    assert.equal(displayed.task, id);
  }
});

test("article verification independently checks the saved title, content, count and unrelated control", () => {
  const prepared = catalog.prepare("publish_article", args);
  for (const mutate of [
    (r) => { r.native_after[0].value = args.content; },
    (r) => { r.native_after[1].value = args.title; },
    (r) => { r.native_after[2].title = args.content; },
    (r) => { r.native_after[2].value = args.title; },
    (r) => { r.native_after[2].clicks = 0; },
    (r) => { r.native_after[2].clicks = 2; },
    (r) => { r.native_after[3].clicks++; },
    (r) => { r.native_after.pop(); },
    (r) => { r.profile = "tauri.taskboard"; },
    (r) => { r.kind = "javascript_task_run"; },
    (r) => { r.effects = 11; },
    (r) => { r.result.value = false; },
  ]) {
    const evidence = report("publish_article"); mutate(evidence);
    assert.equal(prepared.verify(evidence), false, String(mutate));
  }
  assert.equal(catalog.prepare("publish_article", { title: args.content, content: args.title }).verify(report("publish_article")), false);
  for (const id of ["read_title", "read_content", "check_article"]) {
    const evidence = report(id); evidence.native_after[3].clicks++;
    assert.equal(catalog.prepare(id, {}).verify(evidence), false);
    if (id.startsWith("read_")) {
      const forged = report(id); forged.result.value = "invented result";
      assert.equal(catalog.prepare(id, {}).verify(forged), false);
    }
  }
});

test("one shared offline provider discovers form, board and article without application-specific branches", async () => {
  for (const [file, task, inputs] of [
    ["tauri-agent-semantics-v2.json", "read_message", {}],
    ["tauri-board-semantics-v2.json", "read_review_status", {}],
    ["tauri-article-semantics-v3.json", "read_title", {}],
    ["tauri-article-semantics-v3.json", "publish_article", args],
  ]) {
    const value = await fixture(file);
    const message = `task ${task}${Object.keys(inputs).length ? `:${JSON.stringify(inputs)}` : ""}`;
    assert.deepEqual(await provider.plan({ taskId: 1, message, catalog: value }, signal), prepareSemanticCatalog(value).prepare(task, inputs).plan);
  }
});

test("invalid named arguments fail in the shared workflow before application execution", async () => {
  const command = (inputs) => `task publish_article:${JSON.stringify(inputs)}`;
  for (const message of [
    command({ title: "missing content" }), command({ title: "x", content: "y", extra: "z" }),
    command({ title: "x", content: 42 }), command({ title: "x", content: "x".repeat(4097) }),
    'task publish_article:{"title":"first","title":"second","content":"x"}',
    "task publish_article:plain text", 'task read_title:{"title":"unexpected"}',
  ]) {
    let executions = 0;
    const session = new AgentWorkflowSession({
      provider: { async plan(context, signal) { return { steps: [await provider.plan(context, signal)] }; } },
      host: { async describe() { return raw; }, async execute() { executions++; assert.fail("invalid input reached application"); }, verify() { return false; } },
    });
    const outcome = await session.submit(message);
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, false);
    assert.equal(executions, 0);
  }
});
