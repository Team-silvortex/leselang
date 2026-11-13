import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { offlineProvider } from "./dist/offline-provider.js";

const catalog = await readFile(new URL("../../../consumer/examples/catalog_agent/fixtures/tauri-semantics-v1.json", import.meta.url), "utf8");
const signal = () => new AbortController().signal;
const plan = (message, input = catalog, abort = signal()) => offlineProvider.plan({ taskId: 1, message, catalog: input }, abort);

test("application aliases select a host-declared task while preserving literal text", async () => {
  let source;
  for (const text of ["", "保存 ✓", " quotes \" \\ \n第二行", "fn main() = false", "--fault-after-commit"]) {
    for (const prefix of ["保存：", "保存:", "save:", "task fill_and_save:", "任务 fill_and_save："]) {
      const result = await plan(prefix + text);
      assert.equal(result.task, "fill_and_save");
      assert.equal(result.arguments.text, text);
      source ??= result.source;
      assert.equal(result.source, source);
      assert.match(result.source, /gui\.set_text/);
      assert.match(result.source, /gui\.observe/);
    }
  }
});

test("unknown commands, task mismatch, invalid inputs and cancellation never produce a workflow", async () => {
  for (const text of ["请帮我保存", "save something", "Save:hello", " 保存：hello", "上次那条再执行一次", "保存：\0", "save:\ud800", "save:\udc00", `保存：${"x".repeat(4097)}`, "task unknown:hello", "task fill_and_save"]) {
    await assert.rejects(plan(text));
  }
  const aborted = new AbortController(); aborted.abort();
  await assert.rejects(plan("保存：未执行", catalog, aborted.signal));
});

test("the app provider requires declared semantics and never falls back to its old form program", async () => {
  const legacy = await readFile(new URL("fixtures/tauri-catalog.json", import.meta.url), "utf8");
  await assert.rejects(plan("保存：hello", legacy));
  for (const semantics of [null, { version: 999 }]) {
    const value = JSON.parse(legacy); value.semantics = semantics;
    await assert.rejects(plan("保存：hello", value));
  }
  // Even the application's convenience alias cannot invent its target task.
  const changed = catalog.replace('"id":"fill_and_save"', '"id":"another_task"');
  assert.notEqual(changed, catalog);
  await assert.rejects(plan("保存：hello", changed));
  const result = await plan("task another_task:hello", changed);
  assert.equal(result.task, "another_task");
});
