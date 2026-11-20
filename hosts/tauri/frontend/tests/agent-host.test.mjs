import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { createAgentHost } from "../dist/agent/host.js";
import { prepareSemanticCatalog, prepareSemanticWorkflow, AgentWorkflowSession } from "../dist/bridge.js";

const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const catalog = await readFile(new URL("../../../../fixtures/host-semantics/tauri-agent-semantics-v1.json", import.meta.url), "utf8");
const plan = prepareSemanticCatalog(catalog).prepare("fill_and_save", { text: "quote \" \\ \n保存" }).plan;
function evidence(text, beforeClicks = 0) {
  const object = (identifier, role, value, clicks, editable, title) => ({ identifier, role, value, clicks, editable, title, enabled: true });
  return {
    schema_version: 1, kind: "tauri_task_run", profile: "tauri.typescript",
    result: { type: "boolean", value: true }, effects: 9, fuel_remaining: 9000,
    native_before: [object("save", "button", "old", beforeClicks, false, "Save"), object("other", "button", "", 0, false, "Other control"), object("input", "text_field", "old", 0, true, "Text to save")],
    native_after: [object("save", "button", text, beforeClicks + 1, false, "Save"), object("other", "button", "", 0, false, "Other control"), object("input", "text_field", text, 0, true, "Text to save")],
  };
}
async function fixture(catalog_ = catalog) {
  const starts = [], calls = [], replies = [];
  const mutation = deferred();
  let deferReplies = false;
  const api = { core: {
    Channel: class { onmessage = () => {}; },
    invoke(command, args) {
      calls.push({ command, args });
      if (command === "agent_describe") {
        assert.equal(args.raw, true);
        assert.equal(args.taskCatalog, true);
        assert.equal(args.observedResults, true);
        return Promise.resolve(catalog_);
      }
      if (command === "agent_start_task" || command === "agent_start_named_task") {
        const pending = deferred(); starts.push({ ...pending, args }); return pending.promise;
      }
      if (command === "agent_reply" && deferReplies) {
        const pending = deferred(); replies.push(pending); return pending.promise;
      }
      return Promise.resolve();
    },
  } };
  const form = {
    active: null, attached: [], writes: 0, visible: evidence(plan.arguments.text).native_before,
    attach(bridge) {
      assert.equal(this.active, null);
      bridge.register("button", {
        read: () => ({ role: "button", title: "Button", value: "", enabled: true, editable: false, clicks: 0 }),
        click: async (context) => {
          await mutation.promise;
          context.commit(() => { this.writes += 1; });
        },
      });
      this.attached.push(bridge); this.active = bridge;
    },
    // Detach deliberately does not close the bridge. Host lifetime safety must
    // not depend on an application callback remembering that separate duty.
    detach(bridge) { if (this.active === bridge) this.active = null; },
    observe() { return this.visible; },
  };
  const host = createAgentHost(api, form, { epoch: "page-1" });
  await host.describe(new AbortController().signal);
  const request = (start, id = 1) => ({
    transport_version: 1, window: "main", epoch: start.args.epoch,
    session: start.args.session, request_id: id, budget_ms: 1000,
    operation: "gui.list", arguments: {},
  });
  return { host, form, starts, calls, replies, request, mutation, deferReplies() { deferReplies = true; } };
}

test("application host uses fresh scoped bridges and keeps plan data separate across tasks", async () => {
  const f = await fixture();
  assert.equal(await f.host.describe(new AbortController().signal), catalog);
  for (let index = 0; index < 2; index++) {
    const pending = f.host.execute(plan, new AbortController().signal);
    const start = f.starts[index];
    assert.equal(start.args.source, plan.source);
    assert.deepEqual(start.args.arguments, plan.arguments);
    assert.equal(Object.hasOwn(start.args, "text"), false);
    assert.equal(start.args.epoch, "page-1");
    start.args.onRequest.onmessage(f.request(start));
    await tick();
    const reply = f.calls.filter((call) => call.command === "agent_reply").at(-1).args.reply;
    assert.equal(reply.status, "ok");
    assert.equal(reply.session, start.args.session);
    start.resolve({ round: index });
    assert.deepEqual(await pending, { round: index });
    assert.equal(f.form.active, null);
  }
  assert.notEqual(f.starts[0].args.session, f.starts[1].args.session);
});

test("an admitted v3 plan routes to the explicit named-input native command", async () => {
  const description = JSON.parse(catalog);
  const task = description.semantics.tasks.find((task) => task.id === "fill_and_save");
  task.inputs.push({ name: "tag", scalar_type: "string" });
  task.execution = "named_strings";
  description.semantics = { ...description.semantics, version: 3, tasks: [task] };
  const raw = JSON.stringify(description);
  const declared = prepareSemanticCatalog(raw).prepare("fill_and_save", { text: "正文 ✓", tag: "literal label" }).plan;
  const f = await fixture(raw);
  const pending = f.host.execute(declared, new AbortController().signal);
  const start = f.starts[0];
  assert.equal(f.calls.at(-1).command, "agent_start_named_task");
  assert.deepEqual({ ...start.args.arguments }, { text: "正文 ✓", tag: "literal label" });
  assert.equal(start.args.source, declared.source);
  start.resolve({ routing: "named" });
  assert.deepEqual(await pending, { routing: "named" });
  assert.equal(f.calls.some((call) => call.command === "agent_start_task"), false);
  f.host.close();
});

test("cancellation closes the current bridge while awaiting actual host settlement", async () => {
  const f = await fixture();
  const controller = new AbortController();
  const pending = f.host.execute(plan, controller.signal);
  controller.abort();
  assert.equal(f.form.active, null);
  await assert.rejects(f.host.execute(plan, new AbortController().signal));
  const start = f.starts[0];
  start.args.onRequest.onmessage(f.request(start));
  await tick();
  const reply = f.calls.find((call) => call.command === "agent_reply").args.reply;
  assert.equal(reply.status, "rejected");
  const cancelled = f.calls.find((call) => call.command === "agent_cancel");
  assert.deepEqual(cancelled.args, { epoch: "page-1", session: start.args.session });
  start.resolve({ status: "cancelled" });
  await pending;
  const next = f.host.execute(plan, new AbortController().signal);
  f.starts[1].resolve({}); await next;
});

test("a late failed reply from the old channel cannot close the next workflow", async () => {
  const f = await fixture();
  f.deferReplies();
  const first = f.host.execute(plan, new AbortController().signal);
  f.starts[0].args.onRequest.onmessage(f.request(f.starts[0]));
  await tick();
  f.starts[0].resolve({}); await first;
  const second = f.host.execute(plan, new AbortController().signal);
  const current = f.form.active;
  f.replies[0].reject(new Error("old reply rejected"));
  await tick();
  assert.equal(f.form.active, current);
  const cancel = f.calls.filter((call) => call.command === "agent_cancel").at(-1);
  assert.equal(cancel.args.session, f.starts[0].args.session);
  assert.notEqual(cancel.args.session, f.starts[1].args.session);
  f.starts[1].args.onRequest.onmessage(f.request(f.starts[1]));
  await tick();
  assert.equal(f.calls.filter((call) => call.command === "agent_reply").at(-1).args.reply.status, "ok");
  f.replies[1].resolve();
  f.starts[1].resolve({}); await second;
});

test("the host itself retires pending mutations even when application detach only detaches", async () => {
  const f = await fixture();
  const controller = new AbortController();
  const pending = f.host.execute(plan, controller.signal);
  const start = f.starts[0];
  start.args.onRequest.onmessage({
    ...f.request(start), operation: "gui.click",
    arguments: { target: { name: "button", generation: 1 }, expected_revision: 0 },
  });
  await tick();
  controller.abort();
  f.mutation.resolve();
  await tick();
  assert.equal(f.form.writes, 0);
  const reply = f.calls.find((call) => call.command === "agent_reply").args.reply;
  assert.equal(reply.status, "uncertain");
  start.resolve({ status: "cancelled" }); await pending;
});

test("page closure and already-aborted work never admit another task", async () => {
  const f = await fixture();
  const aborted = new AbortController(); aborted.abort();
  await assert.rejects(f.host.execute(plan, aborted.signal));
  assert.equal(f.starts.length, 0);
  const pending = f.host.execute(plan, new AbortController().signal);
  f.host.close(); f.host.close();
  assert.equal(f.form.active, null);
  assert.equal(f.calls.filter((call) => call.command === "agent_closed").length, 1);
  f.starts[0].resolve({ status: "cancelled" }); await pending;
  await assert.rejects(f.host.describe(new AbortController().signal));
  await assert.rejects(f.host.execute(plan, new AbortController().signal));
  assert.equal(f.starts.length, 1);
});

test("semantic verification binds sequential reports to captured and freshly observed application state", async () => {
  const f = await fixture();
  for (let index = 0; index < 2; index++) {
    const text = `task ${index} ✓`;
    const workflow = prepareSemanticCatalog(catalog).prepare("fill_and_save", { text }).plan;
    const report = evidence(text, index);
    report.native_before = structuredClone(f.form.visible);
    const pending = f.host.execute(workflow, new AbortController().signal);
    f.form.visible = structuredClone(report.native_after).reverse();
    f.starts[index].resolve(JSON.stringify(report));
    assert.equal(await pending, JSON.stringify(report));
    assert.equal(f.host.verify(JSON.stringify(report), workflow, new AbortController().signal), true);
    assert.equal(f.host.verify(JSON.stringify(report), workflow, new AbortController().signal), false, "evidence is consumed once");
  }
});

test("provider source and task changes cannot replace the independently described contract", async () => {
  const f = await fixture();
  for (const workflow of [
    { ...plan, source: "fn main(text: string) = true" },
    { ...plan, task: "invented" },
    { summary: plan.summary, source: plan.source, arguments: plan.arguments },
    { ...plan, arguments: { text: "x", extra: "unbound" } },
  ]) {
    await assert.rejects(f.host.execute(workflow, new AbortController().signal));
  }
  assert.equal(f.starts.length, 0);
  assert.equal(f.form.attached.length, 0);
  assert.equal(f.host.verify(evidence(plan.arguments.text), plan, new AbortController().signal), false);
});

test("native carrier admits declared no-input tasks, renamed inputs and distinct result types", async () => {
  for (const [task, arguments_, result] of [
    ["save_message", { message: "renamed input ✓" }, { type: "string", value: "saved" }],
    ["inspect_controls", {}, { type: "integer", value: 3 }],
    ["check_controls", {}, { type: "none", value: null }],
  ]) {
    const f = await fixture();
    const workflow = prepareSemanticCatalog(catalog).prepare(task, arguments_).plan;
    const report = evidence(arguments_.message ?? "old");
    report.result = result;
    if (!Object.keys(arguments_).length) {
      report.effects = 3;
      report.native_after = structuredClone(report.native_before);
    }
    const pending = f.host.execute(workflow, new AbortController().signal);
    assert.deepEqual({ ...f.starts[0].args.arguments }, arguments_);
    assert.equal(Object.hasOwn(f.starts[0].args, "text"), false);
    f.form.visible = structuredClone(report.native_after);
    f.starts[0].resolve(JSON.stringify(report));
    const received = await pending;
    assert.equal(received, JSON.stringify(report));
    assert.equal(f.host.verify(received, workflow, new AbortController().signal), true, task);
  }
});

test("raw execution reports preserve exact integers and bind the original evidence bytes", async () => {
  const exact = "18446744073709551615";
  const description = catalog
    .replace('"result":{"kind":"integer","value":3}', `"result":{"kind":"integer","value":${exact}}`)
    .replace('"result":{"kind":"literal","value":{"kind":"integer","value":3}}', `"result":{"kind":"literal","value":{"kind":"integer","value":${exact}}}`);
  const workflow = prepareSemanticCatalog(description).prepare("inspect_controls", {}).plan;
  for (const candidate of ["same", "rounded", "reencoded", "object"]) {
    const f = await fixture(description);
    const report = evidence("old");
    report.result = { type: "integer", value: 3 };
    report.effects = 3;
    report.native_after = structuredClone(report.native_before);
    const raw = JSON.stringify(report).replace('"result":{"type":"integer","value":3}', `"result":{"type":"integer","value":${exact}}`);
    const pending = f.host.execute(workflow, new AbortController().signal);
    f.starts[0].resolve(raw);
    assert.equal(await pending, raw);
    const changed = candidate === "rounded" ? raw.replace(exact, "18446744073709551614")
      : candidate === "reencoded" ? `${raw}\n` : candidate === "object" ? JSON.parse(raw) : raw;
    assert.equal(f.host.verify(changed, workflow, new AbortController().signal), candidate === "same", candidate);
  }
});

test("invalid or unencoded transport reports cannot establish completed evidence", async () => {
  const report = evidence(plan.arguments.text);
  const raw = JSON.stringify(report);
  for (const received of [report, "not json", raw.replace('"effects":9', '"effects":8,"effects":9'),
    JSON.stringify({ schema_version: 1, kind: "tauri_task_run", status: "cancelled", possible_partial_effects: true })]) {
    const f = await fixture();
    const pending = f.host.execute(plan, new AbortController().signal);
    f.form.visible = structuredClone(report.native_after);
    f.starts[0].resolve(received);
    await pending;
    assert.equal(f.host.verify(raw, plan, new AbortController().signal), false);
  }
});

test("a report cannot replace its execution, earlier observation, or current visible state", async () => {
  for (const change of ["earlier", "current", "duplicate", "extra", "different_report", "different_plan"]) {
    const f = await fixture();
    const report = evidence(plan.arguments.text);
    if (change === "earlier") {
      // This satisfies the declared unchanged-title condition, but never
      // matches the actual state captured before starting the application.
      report.native_before[2].title = "other title";
      report.native_after[2].title = "other title";
    }
    const pending = f.host.execute(plan, new AbortController().signal);
    f.form.visible = structuredClone(report.native_after);
    f.starts[0].resolve(JSON.stringify(report)); await pending;
    let candidate = structuredClone(report), workflow = plan;
    if (change === "current") f.form.visible[0].value = "manual edit";
    if (change === "duplicate") f.form.visible[1].identifier = f.form.visible[0].identifier;
    if (change === "extra") f.form.visible[1].extra = true;
    if (change === "different_report") candidate.fuel_remaining -= 1;
    if (change === "different_plan") workflow = { ...plan, arguments: { text: "different" } };
    assert.equal(f.host.verify(JSON.stringify(candidate), workflow, new AbortController().signal), false, change);
  }
});

test("declared postcondition failures cannot be replaced by truthy results", async () => {
  const changes = [
    (r) => { r.result.value = false; }, (r) => { r.result.type = "string"; },
    (r) => { r.effects = 8; }, (r) => { r.fuel_remaining = 10_001; },
    (r) => { r.kind = "javascript_run"; }, (r) => { r.profile = "javascript.registered"; },
    (r) => { r.native_after[0].clicks += 1; }, (r) => { r.native_after[0].value = "wrong"; },
    (r) => { r.native_after[2].value = "wrong"; }, (r) => { r.native_after[1].clicks += 1; },
    (r) => { r.native_before[0].clicks = -1; r.native_after[0].clicks = 0; },
    (r) => { r.native_before[0].role = "text_field"; },
    (r) => { r.native_before[0].enabled = false; },
    (r) => { r.native_before[1].value = {}; },
    (r) => { r.native_after[0].identifier = "input"; },
  ];
  for (const change of changes) {
    const f = await fixture();
    const report = evidence(plan.arguments.text); change(report);
    const pending = f.host.execute(plan, new AbortController().signal);
    f.form.visible = structuredClone(report.native_after);
    f.starts[0].resolve(JSON.stringify(report)); await pending;
    assert.equal(f.host.verify(JSON.stringify(report), plan, new AbortController().signal), false);
  }
});

test("closed or cancelled executions never publish late semantic verification", async () => {
  for (const close of [false, true]) {
    const f = await fixture();
    const report = evidence(plan.arguments.text);
    const controller = new AbortController();
    const pending = f.host.execute(plan, controller.signal);
    if (close) f.host.close(); else controller.abort();
    f.form.visible = report.native_after;
    f.starts[0].resolve(JSON.stringify(report)); await pending;
    assert.equal(f.host.verify(JSON.stringify(report), plan, new AbortController().signal), false);
  }
});

test("registered-control observation types and u32 counts remain enforced without form names", async () => {
  for (const mutate of [
    (rows) => { rows[1].extra = true; },
    (rows) => { rows[1].enabled = 1; },
    (rows) => { rows[1].editable = "false"; },
    (rows) => { rows[1].title = {}; },
    (rows) => { rows[1].value = "x".repeat(4097); },
    (rows) => { rows[1].clicks = 0x1_0000_0000; },
    (rows) => { rows[1].clicks = -1; },
  ]) {
    const f = await fixture(); mutate(f.form.visible);
    await assert.rejects(f.host.execute(plan, new AbortController().signal));
    assert.equal(f.starts.length, 0);
    assert.equal(f.form.attached.length, 0);
  }
  const f = await fixture();
  const report = evidence(plan.arguments.text, 0xffff_ffff);
  f.form.visible = structuredClone(report.native_before);
  const pending = f.host.execute(plan, new AbortController().signal);
  f.form.visible = structuredClone(report.native_after);
  f.starts[0].resolve(JSON.stringify(report)); await pending;
  assert.equal(f.host.verify(JSON.stringify(report), plan, new AbortController().signal), false);
});

test("application teardown failure cannot leave verifiable completion evidence", async () => {
  const f = await fixture();
  const report = evidence(plan.arguments.text);
  const detach = f.form.detach;
  f.form.detach = function(bridge) { detach.call(this, bridge); throw new Error("detach failed"); };
  const pending = f.host.execute(plan, new AbortController().signal);
  f.form.visible = report.native_after;
  f.starts[0].resolve(JSON.stringify(report));
  await assert.rejects(pending, /detach failed/);
  assert.equal(f.host.verify(JSON.stringify(report), plan, new AbortController().signal), false);
  assert.equal(f.form.active, null);
});

test("execution cancellation after settlement fences later verification with a different signal", async () => {
  const f = await fixture();
  const report = evidence(plan.arguments.text);
  const controller = new AbortController();
  const pending = f.host.execute(plan, controller.signal);
  f.form.visible = structuredClone(report.native_after);
  f.starts[0].resolve(JSON.stringify(report));
  await pending;
  // execute has already removed its active abort listener. The completed
  // evidence must still retain the original execution's cancellation fence.
  controller.abort();
  assert.equal(f.host.verify(JSON.stringify(report), plan, new AbortController().signal), false);
});

test("workflow composition uses fresh application evidence and stops after a later failed verification", async () => {
  const semantic = prepareSemanticCatalog(catalog);
  const plans = [semantic.prepare("fill_and_save", { text: "workflow ✓" }).plan,
    semantic.prepare("inspect_controls", {}).plan, semantic.prepare("check_controls", {}).plan];
  for (const failedSecond of [false, true]) {
    const f = await fixture();
    const session = new AgentWorkflowSession({ provider: { plan: async () => ({ steps: plans }) }, host: f.host });
    const pending = session.submit("save, inspect, check");
    let previous;
    const count = failedSecond ? 2 : 3;
    for (let index = 0; index < count; index++) {
      for (let turn = 0; !f.starts[index] && turn < 20; turn++) await tick();
      assert.ok(f.starts[index], "next execution is admitted after prior verification");
      assert.equal(f.starts[index].args.source, plans[index].source);
      const report = index === 0 ? evidence("workflow ✓") : {
        ...previous, effects: 3, native_before: previous.native_after, native_after: previous.native_after,
        result: index === 1 ? { type: "integer", value: failedSecond ? 999 : 3 } : { type: "none", value: null },
      };
      f.form.visible = structuredClone(report.native_after);
      f.starts[index].resolve(JSON.stringify(report));
      previous = report;
    }
    const outcome = await pending;
    assert.equal(outcome.status, failedSecond ? "failed" : "verified");
    assert.equal(outcome.possiblePartialEffects, failedSecond);
    assert.equal(outcome.steps.length, count);
    assert.equal(outcome.steps[0].status, "verified");
    assert.equal(f.starts.length, count, "a failed verification does not start the remaining task");
    assert.equal(new Set(f.starts.map((start) => start.args.session)).size, count);
    assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, 1);
    assert.equal(f.form.visible.find((row) => row.identifier === "save").value, "workflow ✓");
    assert.equal(f.form.active, null);
  }
});

test("workflow cancellation at a verified application boundary keeps the save and never attaches another bridge", async () => {
  const f = await fixture();
  let session;
  session = new AgentWorkflowSession({
    provider: { plan: async () => prepareSemanticWorkflow(catalog, [
      { task: "fill_and_save", arguments: plan.arguments },
      { task: "save_message", arguments: { message: { fromStep: 0, format: "boolean" } } },
    ]) },
    host: f.host,
    onEvent: (state) => { if (state.completedSteps === 1) session.cancel(); },
  });
  const pending = session.submit("save then inspect");
  for (let turn = 0; !f.starts[0] && turn < 20; turn++) await tick();
  const report = evidence(plan.arguments.text);
  f.form.visible = report.native_after;
  f.starts[0].resolve(JSON.stringify(report));
  const outcome = await pending;
  assert.equal(outcome.status, "cancelled");
  assert.equal(outcome.possiblePartialEffects, true);
  assert.equal(outcome.steps[0].status, "verified");
  assert.equal(f.starts.length, 1);
  assert.equal(f.form.attached.length, 1);
  assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, 1);
  assert.equal(f.form.active, null);
});

test("verified control count becomes the next actual host argument without interpolating source", async () => {
  const f = await fixture();
  const workflow = prepareSemanticWorkflow(catalog, [
    { task: "inspect_controls", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "decimal" } } },
    { task: "check_controls", arguments: {} },
  ]);
  const session = new AgentWorkflowSession({ provider: { plan: async () => workflow }, host: f.host });
  const pending = session.submit("inspect count then save it");
  const initial = evidence("old").native_before;
  const saved = evidence("3");
  const reports = [
    { ...saved, result: { type: "integer", value: 3 }, effects: 3, native_before: initial, native_after: initial },
    { ...saved, result: { type: "string", value: "saved" } },
    { ...saved, result: { type: "none", value: null }, effects: 3, native_before: saved.native_after },
  ];
  for (let index = 0; index < 3; index++) {
    for (let turn = 0; !f.starts[index] && turn < 20; turn++) await tick();
    assert.ok(f.starts[index]);
    assert.equal(f.starts[index].args.source, workflow.steps[index].source);
    if (index === 1) assert.deepEqual({ ...f.starts[index].args.arguments }, { message: "3" });
    f.form.visible = structuredClone(reports[index].native_after);
    f.starts[index].resolve(JSON.stringify(reports[index]));
  }
  const outcome = await pending;
  assert.equal(outcome.status, "verified");
  assert.equal(outcome.plan.steps[1].arguments.message, "", "the admitted template stays immutable");
  assert.equal(outcome.steps[1].plan.arguments.message, "3", "actual resolved argument is retained");
  assert.equal(f.form.visible.find((row) => row.identifier === "save").value, "3");
  assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, 1);
  assert.equal(f.form.active, null);
});

const observedCatalog = await readFile(new URL("../../../../fixtures/host-semantics/tauri-agent-semantics-v2.json", import.meta.url), "utf8");
function observedWorkflow() {
  return prepareSemanticWorkflow(observedCatalog, [
    { task: "read_message", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "string" } } },
    { task: "check_controls", arguments: {} },
  ]);
}
function readEvidence(visible) {
  return {
    schema_version: 1, kind: "tauri_task_run", profile: "tauri.typescript",
    result: { type: "string", value: visible.find((row) => row.identifier === "input").value },
    effects: 2, fuel_remaining: 9000,
    native_before: structuredClone(visible), native_after: structuredClone(visible),
  };
}
async function waitForStart(f, index) {
  for (let turn = 0; !f.starts[index] && turn < 20; turn++) await tick();
  assert.ok(f.starts[index], "execution must start only after previous verification");
  return f.starts[index];
}

test("one v2 catalog and unchanged source read different live inputs into verified save arguments", async () => {
  const f = await fixture(observedCatalog);
  const workflow = observedWorkflow();
  const session = new AgentWorkflowSession({ provider: { plan: async ({ catalog: current }) => {
    assert.equal(current, observedCatalog);
    return workflow;
  } }, host: f.host });
  let startIndex = 0;
  for (const value of ['现场读取甲 "✓" \\ \n文本', '{"fromStep":7,"format":"string"}']) {
    f.form.visible.find((row) => row.identifier === "input").value = value;
    const pending = session.submit("read current text, save it, then check");
    const read = readEvidence(f.form.visible);
    (await waitForStart(f, startIndex)).resolve(JSON.stringify(read));
    const save = await waitForStart(f, startIndex + 1);
    assert.deepEqual({ ...save.args.arguments }, { message: value });
    assert.equal(save.args.source, workflow.steps[1].source);
    const saved = structuredClone(f.form.visible);
    saved.find((row) => row.identifier === "save").value = value;
    saved.find((row) => row.identifier === "save").clicks += 1;
    const report = { ...read, result: { type: "string", value: "saved" }, effects: 9, native_after: saved };
    f.form.visible = structuredClone(saved);
    save.resolve(JSON.stringify(report));
    const check = await waitForStart(f, startIndex + 2);
    check.resolve(JSON.stringify({ ...report, result: { type: "none", value: null }, effects: 3, native_before: saved }));
    const outcome = await pending;
    assert.equal(outcome.status, "verified");
    assert.equal(JSON.parse(outcome.steps[0].report).result.value, value);
    assert.equal(outcome.steps[1].plan.arguments.message, value);
    assert.equal(outcome.plan.steps[1].arguments.message, "");
    assert.equal(f.form.visible.find((row) => row.identifier === "save").value, value);
    assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, startIndex / 3 + 1);
    assert.equal(f.form.visible.find((row) => row.identifier === "other").clicks, 0);
    startIndex += 3;
  }
  assert.equal(new Set(f.starts.map((start) => start.args.session)).size, 6);
});

test("dynamic observation needs matching report evidence and independent current application state", async () => {
  for (const fault of ["result", "wrong_type", "missing", "ambiguous", "invented_initial", "changed_live", "unrelated_mutation"]) {
    const f = await fixture(observedCatalog);
    const workflow = observedWorkflow();
    const session = new AgentWorkflowSession({ provider: { plan: async () => workflow }, host: f.host });
    const pending = session.submit("never save unverified observation");
    const start = await waitForStart(f, 0);
    const report = readEvidence(f.form.visible);
    const input = (rows) => rows.find((row) => row.identifier === "input");
    if (fault === "result") report.result.value = "invented";
    if (fault === "wrong_type") report.result = { type: "boolean", value: true };
    if (fault === "missing") report.native_after = report.native_after.filter((row) => row.identifier !== "input");
    if (fault === "ambiguous") report.native_after.push(structuredClone(input(report.native_after)));
    if (fault === "invented_initial") {
      report.result.value = "invented";
      input(report.native_before).value = "invented";
      input(report.native_after).value = "invented";
      input(f.form.visible).value = "invented";
    }
    if (fault === "changed_live") input(f.form.visible).value = "changed after observation";
    if (fault === "unrelated_mutation") {
      report.native_after.find((row) => row.identifier === "other").clicks += 1;
      f.form.visible = structuredClone(report.native_after);
    }
    start.resolve(JSON.stringify(report));
    const outcome = await pending;
    assert.equal(outcome.status, "failed", fault);
    assert.equal(outcome.error, "application_verification_failed", fault);
    assert.equal(f.starts.length, 1, fault);
    assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, 0, fault);
  }
});

test("a cancelled dynamic read cannot feed a later save after its report arrives", async () => {
  const f = await fixture(observedCatalog);
  const session = new AgentWorkflowSession({ provider: { plan: async () => observedWorkflow() }, host: f.host });
  const pending = session.submit("read then save");
  const start = await waitForStart(f, 0);
  assert.equal(session.cancel(), true);
  assert.equal(session.busy, true);
  await assert.rejects(session.submit("overlap"), /agent_task_pending/);
  start.resolve(JSON.stringify(readEvidence(f.form.visible)));
  const outcome = await pending;
  assert.equal(outcome.status, "cancelled");
  assert.equal(f.starts.length, 1);
  assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, 0);
  assert.equal(f.form.active, null);
});

test("verified application observations gate later native execution and retain mismatched evidence", async () => {
  const expected = "条件执行回归 ✓";
  const workflow = prepareSemanticWorkflow(observedCatalog, [
    { task: "read_message", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "string" } },
      requires: { fromStep: 0, format: "string", equals: expected } },
    { task: "check_controls", arguments: {} },
  ]);
  for (const mode of ["matches", "mismatch", "changed_live"]) {
    const f = await fixture(observedCatalog);
    const actual = mode === "mismatch" ? "condition not satisfied" : expected;
    f.form.visible.find((row) => row.identifier === "input").value = actual;
    const before = structuredClone(f.form.visible);
    const session = new AgentWorkflowSession({ provider: { plan: async () => workflow }, host: f.host });
    const pending = session.submit("read, require the expected value, save and check");
    const start = await waitForStart(f, 0);
    const read = readEvidence(f.form.visible);
    if (mode === "changed_live") f.form.visible.find((row) => row.identifier === "input").value = "manual edit after observation";
    start.resolve(JSON.stringify(read));
    if (mode === "matches") {
      const save = await waitForStart(f, 1);
      assert.deepEqual({ ...save.args.arguments }, { message: actual });
      assert.equal(save.args.source, workflow.steps[1].source);
      const saved = structuredClone(before);
      saved.find((row) => row.identifier === "save").value = actual;
      saved.find((row) => row.identifier === "save").clicks += 1;
      const report = { ...read, effects: 9, result: { type: "string", value: "saved" }, native_after: saved };
      f.form.visible = structuredClone(saved);
      save.resolve(JSON.stringify(report));
      (await waitForStart(f, 2)).resolve(JSON.stringify({ ...report, effects: 3,
        result: { type: "none", value: null }, native_before: saved }));
    }
    const outcome = await pending;
    if (mode === "changed_live") {
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.error, "application_verification_failed");
      assert.equal(outcome.steps[0].status, "failed");
      assert.equal(outcome.requirements, undefined, "unverified observation cannot reach the comparison");
    } else {
      assert.equal(outcome.status, mode === "matches" ? "verified" : "failed");
      assert.equal(outcome.steps[0].status, "verified");
      assert.deepEqual(outcome.requirements, [{ step: 1, fromStep: 0, format: "string", equals: expected,
        actual, satisfied: mode === "matches" }]);
      if (mode === "mismatch") {
        assert.equal(outcome.error, "workflow_requirement_not_met");
        assert.equal(outcome.stoppedStep, 1);
        assert.equal(outcome.steps[1].report, undefined);
        assert.equal(outcome.steps[1].possiblePartialEffects, false);
        assert.deepEqual(f.form.visible, before, "unmet condition performs no mutation");
      }
    }
    const executions = mode === "matches" ? 3 : 1;
    assert.equal(f.starts.length, executions, "the native target starts only after a satisfied condition");
    assert.equal(f.form.attached.length, executions);
    assert.equal(outcome.possiblePartialEffects, mode !== "matches", "failed workflows conservatively retain prior execution");
    assert.equal(f.form.visible.find((row) => row.identifier === "save").clicks, mode === "matches" ? 1 : 0);
    assert.equal(f.form.visible.find((row) => row.identifier === "other").clicks, 0);
    assert.equal(f.form.active, null);
  }
});
