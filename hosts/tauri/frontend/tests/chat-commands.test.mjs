import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { taskCommand } from "../dist/agent/chat.js";
import { prepareSemanticCatalog, createSemanticProvider } from "../dist/bridge.js";

const legacy = await readFile(new URL("../../../../fixtures/host-semantics/tauri-agent-semantics-v1.json", import.meta.url), "utf8");
const provider = createSemanticProvider();
const select = (catalog, message) => provider.plan({ taskId: 1, catalog, message }, new AbortController().signal);

test("displayed v3 commands preserve every declared input name and round-trip through the offline provider", async () => {
  const base = JSON.parse(legacy);
  const original = base.semantics.tasks.find((task) => task.id === "fill_and_save");
  const inspection = base.semantics.tasks.find((task) => task.id === "inspect_controls");
  const tasks = [
    { ...inspection, id: "inspect", execution: "no_text" },
    ...[["text"], ["title", "text"], ["text", "a", "b", "c", "d", "e", "f", "g"]]
      .map((names, index) => ({ ...original, id: `named_${index}`, execution: "named_strings",
        inputs: names.map((name) => ({ name, scalar_type: "string" })) })),
  ];
  const raw = JSON.stringify({ ...base, semantics: { ...base.semantics, version: 3, tasks } });
  const catalog = prepareSemanticCatalog(raw);
  for (const task of catalog.tasks) {
    const command = taskCommand(task);
    const result = await select(raw, command);
    assert.equal(result.task, task.id);
    assert.equal(result.inputMode, "named_strings");
    assert.deepEqual({ ...result.arguments }, Object.fromEntries(task.inputNames.map((name) => [name, "文本"])));
    assert.equal(command, task.inputNames.length ? `task ${task.id}:${JSON.stringify(result.arguments)}` : `task ${task.id}`);
  }
  assert.equal(taskCommand(catalog.tasks[2]), 'task named_1:{"title":"文本","text":"文本"}');
});

test("legacy help commands retain literal single-text and no-input syntax", async () => {
  const catalog = prepareSemanticCatalog(legacy);
  const fill = catalog.tasks.find((task) => task.id === "fill_and_save");
  const inspect = catalog.tasks.find((task) => task.id === "inspect_controls");
  assert.equal(taskCommand(fill), "task fill_and_save:文本");
  assert.equal(taskCommand(inspect), "task inspect_controls");
  const selected = await select(legacy, taskCommand(fill));
  assert.deepEqual({ ...selected.arguments }, { text: "文本" });
  assert.equal(Object.hasOwn(selected, "inputMode"), false);
  assert.deepEqual({ ...(await select(legacy, taskCommand(inspect))).arguments }, {});
});
