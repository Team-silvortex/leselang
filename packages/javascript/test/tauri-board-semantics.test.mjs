import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { AgentWorkflowSession, createModelWorkflowProvider, createSemanticProvider, prepareSemanticCatalog } from "@leselang/bridge";

const fixture = (name) => readFile(new URL(`../../../fixtures/host-semantics/${name}`, import.meta.url), "utf8");
const raw = await fixture("tauri-board-semantics-v2.json");
const formRaw = await fixture("tauri-agent-semantics-v2.json");
const catalog = prepareSemanticCatalog(raw);
const tasks = [
  ["read_review_status", "read-review", 2], ["read_archive_status", "read-archive", 2],
  ["complete_review", "complete-review", 4], ["complete_archive", "complete-archive", 4],
  ["check_board", "check", 1],
];
function controls(done = []) {
  return [["review", "核对示例报告"], ["archive", "整理示例资料"]].map(([identifier, title]) => ({
    identifier, title, role: "button", value: done.includes(identifier) ? "done" : "pending",
    enabled: !done.includes(identifier), editable: false, clicks: done.includes(identifier) ? 1 : 0,
  }));
}
function report(task, before = controls()) {
  const native_before = structuredClone(before), native_after = structuredClone(before);
  const target = native_after.find((row) => row.identifier === (task.endsWith("archive") || task.includes("archive_status") ? "archive" : "review"));
  if (task.startsWith("complete_")) { target.value = "done"; target.enabled = false; target.clicks++; }
  return { schema_version: 1, kind: "tauri_task_run", profile: "tauri.taskboard",
    result: task === "check_board" ? { type: "none", value: null } : { type: "string", value: target.value },
    effects: tasks.find(([id]) => id === task)[2], fuel_remaining: 9000, native_before, native_after };
}

test("the second application declares distinct tasks with canonical Rust-consumed programs", async () => {
  assert.equal(catalog.profile, "tauri.taskboard");
  assert.deepEqual(catalog.tasks.map((t) => t.id), tasks.map(([id]) => id));
  assert.ok(catalog.tasks.every((t) => t.inputName === undefined));
  for (const [id, name, effects] of tasks) {
    const prepared = catalog.prepare(id, {});
    assert.equal(prepared.plan.source, await fixture(`tauri-board-${name}.lese`));
    assert.equal(prepared.expectedEffects, effects);
    assert.equal(prepared.verify(report(id)), true, id);
    assert.throws(() => catalog.prepare(id, { text: "unexpected" }));
    if (!id.startsWith("complete_")) assert.equal(prepared.verify(report(id, controls(["review", "archive"]))), true);
  }
  assert.throws(() => catalog.prepare("save_message", { message: "form only" }));
  assert.throws(() => prepareSemanticCatalog(formRaw).prepare("complete_review", {}));
});

test("completion contracts reject repeat execution, unrelated mutations and fabricated status transitions", () => {
  for (const task of ["complete_review", "complete_archive"]) {
    const prepared = catalog.prepare(task, {}), id = task.replace("complete_", "");
    const target = (r, side) => r[side].find((row) => row.identifier === id);
    const other = (r, side) => r[side].find((row) => row.identifier !== id);
    const mutations = [
      (r) => { target(r, "native_before").enabled = false; },
      (r) => { target(r, "native_before").value = "done"; },
      (r) => { target(r, "native_before").clicks = 1; },
      (r) => { target(r, "native_after").enabled = true; },
      (r) => { target(r, "native_after").value = "pending"; },
      (r) => { target(r, "native_after").clicks = 2; },
      (r) => { target(r, "native_after").title = "forged title"; },
      (r) => { other(r, "native_after").value = "done"; },
      (r) => { other(r, "native_after").clicks = 1; },
      (r) => { r.native_after.push(structuredClone(target(r, "native_after"))); },
      (r) => { r.native_before.pop(); },
      (r) => { r.result.value = "failed"; },
      (r) => { r.profile = "tauri.typescript"; },
      (r) => { r.effects = 3; },
      (r) => { r.fuel_remaining = 10001; },
    ];
    for (const mutate of mutations) {
      const evidence = report(task); mutate(evidence);
      assert.equal(prepared.verify(evidence), false, `${task}: ${mutate}`);
    }
    assert.equal(prepared.verify(report(task, controls([id]))), false);
    const evidence = report(task); evidence.native_after.reverse();
    assert.equal(prepared.verify(evidence), true, "unique object identifiers do not depend on array order");
  }
});

test("read and check reports require unchanged complete observations and independently matched results", () => {
  for (const task of ["read_review_status", "read_archive_status", "check_board"]) {
    const prepared = catalog.prepare(task, {});
    for (const mutate of [
      (r) => { r.native_after[0].clicks = 1; },
      (r) => { r.native_before[0].clicks = r.native_after[0].clicks = 2; },
      (r) => { r.native_before[0].editable = r.native_after[0].editable = true; },
      (r) => { r.native_before[1].role = r.native_after[1].role = "text_field"; },
      (r) => { r.native_before[1].title = r.native_after[1].title = "different task"; },
      (r) => { r.native_after.pop(); },
      (r) => { r.result.value = task === "check_board" ? false : "invented"; },
    ]) {
      const evidence = report(task); mutate(evidence);
      assert.equal(prepared.verify(evidence), false, `${task}: ${mutate}`);
    }
  }
});

test("one unchanged provider instance discovers and plans both form and board catalogs", async () => {
  const offline = createSemanticProvider();
  const abort = new AbortController().signal;
  for (const [value, task, args] of [[raw, "read_review_status", {}], [formRaw, "read_message", {}]]) {
    const result = await offline.plan({ taskId: 1, message: `task ${task}`, catalog: value }, abort);
    assert.deepEqual(result, prepareSemanticCatalog(value).prepare(task, args).plan);
  }
  const seen = [];
  const provider = createModelWorkflowProvider({ resultBindings: true, resultRequirements: true,
    async complete(request) {
      const context = JSON.parse(request.user);
      seen.push(JSON.parse(context.catalog).profile);
      // Protocol fixture chooses data by the request; production provider code is unchanged.
      return context.message;
    },
  });
  for (const [value, selections] of [[formRaw, [{ task: "read_message", arguments: {} }]],
    [raw, [{ task: "read_review_status", arguments: {} }, { task: "complete_review", arguments: {},
      requires: { fromStep: 0, format: "string", equals: "pending" } }]]]) {
    const result = await provider.plan({ taskId: 1, message: JSON.stringify({ steps: selections }), catalog: value }, abort);
    assert.deepEqual(result.steps.map((p) => p.task), selections.map((p) => p.task));
  }
  assert.deepEqual(seen, ["tauri.typescript", "tauri.taskboard"]);
});

test("one board workflow verifies completion once then stops a repeated instruction before another click", async () => {
  let current = controls();
  const executed = [];
  const provider = createModelWorkflowProvider({ resultRequirements: true, async complete() {
    return JSON.stringify({ steps: [
      { task: "read_review_status", arguments: {} },
      { task: "complete_review", arguments: {}, requires: { fromStep: 0, format: "string", equals: "pending" } },
      { task: "read_review_status", arguments: {} }, { task: "check_board", arguments: {} },
    ] });
  } });
  const session = new AgentWorkflowSession({ provider, host: {
    async describe() { return raw; },
    async execute(plan) {
      executed.push(plan.task);
      const evidence = report(plan.task, current);
      current = structuredClone(evidence.native_after);
      return JSON.stringify(evidence);
    },
    verify(evidence, plan) {
      return catalog.prepare(plan.task, plan.arguments).verify(evidence)
        && JSON.stringify(JSON.parse(evidence).native_after) === JSON.stringify(current);
    },
  } });
  const first = await session.submit("finish the report task only if pending");
  assert.equal(first.status, "verified");
  assert.deepEqual(executed, ["read_review_status", "complete_review", "read_review_status", "check_board"]);
  assert.equal(JSON.parse(first.steps[0].report).result.value, "pending");
  assert.equal(JSON.parse(first.steps[2].report).result.value, "done");
  const second = await session.submit("finish the report task only if pending");
  assert.equal(second.error, "workflow_requirement_not_met");
  assert.equal(second.requirements[0].actual, "done");
  assert.equal(second.steps[1].report, undefined);
  assert.equal(executed.length, 5);
  assert.deepEqual(current, controls(["review"]));
});
