import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { AgentSession, AgentWorkflowSession } from "../dist/bridge.js";
import { createTauriModelProvider, createTauriModelWorkflowProvider, modelInfo } from "../dist/model-provider.js";

const catalog = await readFile(new URL("../../../../fixtures/host-semantics/tauri-agent-semantics-v1.json", import.meta.url), "utf8");
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function fixture() {
  const calls = [], pending = [], executed = [];
  const api = { core: { invoke(command, args) {
    calls.push({ command, args });
    if (command === "agent_model_complete") {
      const result = deferred(); pending.push(result); return result.promise;
    }
    return Promise.resolve();
  } } };
  const provider = createTauriModelProvider(api, { epoch: "page-model" });
  const session = new AgentSession({ provider, host: {
    describe: async () => catalog,
    execute: async (plan) => { executed.push(plan); return { observed: true }; },
    verify: async () => true,
  } });
  return { api, provider, calls, pending, executed, session };
}

test("model configuration IPC exposes only availability and a bounded display name", async () => {
  for (const expected of [{ configured: false }, { configured: true, model: "local-test" }]) {
    assert.deepEqual(await modelInfo({ core: { invoke: async (command) => {
      assert.equal(command, "agent_model_info"); return expected;
    } } }), expected);
  }
  for (const value of [null, [], {}, { configured: 1 }, { configured: true },
    { configured: false, model: "unexpected" }, { configured: true, model: "secret", apiKey: "secret" },
    { configured: true, model: "\n" }, { configured: true, model: "x".repeat(129) },
    { configured: true, model: "模".repeat(43) }]) {
    await assert.rejects(modelInfo({ core: { invoke: async () => value } }));
  }
});

test("natural language travels to native planning and its admitted task reaches execution", async () => {
  const f = fixture();
  const message = "请把这段文字保存起来：计划 ✓";
  const result = f.session.submit(message);
  await tick();
  const request = f.calls[0];
  assert.equal(request.command, "agent_model_complete");
  assert.equal(request.args.epoch, "page-model");
  assert.match(request.args.requestId, /^[0-9a-f-]{36}$/u);
  assert.match(request.args.request.user, /计划 ✓/u);
  assert.deepEqual(Object.keys(request.args).sort(), ["epoch", "request", "requestId"]);
  f.pending[0].resolve(JSON.stringify({ task: "save_message", arguments: { message: "计划 ✓" } }));
  assert.equal((await result).status, "verified");
  assert.equal(f.executed[0].arguments.message, "计划 ✓");
  assert.equal(f.executed[0].task, "save_message");
  assert.ok(!f.executed[0].source.includes("计划 ✓"));
});

test("cancel acknowledgement keeps the task busy until native planning settles; a late plan never executes", async () => {
  const f = fixture();
  const result = f.session.submit("看看应用有多少控件");
  await tick();
  assert.equal(f.session.cancel(), true);
  await tick();
  const start = f.calls[0].args;
  assert.deepEqual(f.calls[1], { command: "agent_model_cancel", args: { epoch: start.epoch, requestId: start.requestId } });
  assert.equal(f.session.busy, true);
  await assert.rejects(f.session.submit("下一条"), /agent_task_pending/u);
  f.pending[0].resolve('{"task":"inspect_controls","arguments":{}}');
  assert.equal((await result).status, "cancelled");
  assert.equal(f.executed.length, 0);
  const next = f.session.submit("查看控件");
  await tick();
  assert.notEqual(f.calls[2].args.requestId, start.requestId);
  f.pending[1].resolve('{"task":"inspect_controls","arguments":{}}');
  assert.equal((await next).status, "verified");
  assert.equal(f.executed.length, 1);
  assert.equal(f.calls.filter((call) => call.command === "agent_model_cancel").length, 1);
});

test("transport failures, refusals and invalid plans do not execute, retry, fall back or echo secrets", async () => {
  for (const failure of [new Error("secret-token https://private.example"),
    { content: 123 }, { content: '{"error":"not supported"}' },
    { content: '{"task":"invented","arguments":{}}' },
    { content: '{"task":"inspect_controls","arguments":{},"source":"fn main() = true"}' }]) {
    const f = fixture();
    const result = f.session.submit("保存：不应离线执行");
    await tick();
    if (failure instanceof Error) f.pending[0].reject(failure);
    else f.pending[0].resolve(failure.content);
    const outcome = await result;
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, false);
    assert.equal(f.executed.length, 0);
    assert.equal(f.calls.length, 1);
    assert.doesNotMatch(outcome.error, /secret-token|private\.example/u);
  }
});

test("an already cancelled request does not enter IPC and failed cancel transport cannot authorize late work", async () => {
  const f = fixture();
  const controller = new AbortController(); controller.abort();
  await assert.rejects(f.provider.plan({ taskId: 1, message: "查看", catalog }, controller.signal));
  assert.equal(f.calls.length, 0);
  const invoke = f.api.core.invoke;
  f.api.core.invoke = (command, args) => command === "agent_model_cancel"
    ? Promise.reject(new Error("cancel disconnected")) : invoke(command, args);
  const result = f.session.submit("查看控件");
  await tick(); f.session.cancel(); await tick();
  f.pending[0].resolve('{"task":"inspect_controls","arguments":{}}');
  assert.equal((await result).status, "cancelled");
  assert.equal(f.executed.length, 0);
});

test("one native planning request drives a sequence and all steps are admitted before any work", async () => {
  for (const invalid of [false, true]) {
    const calls = [], executed = [], verified = [];
    const provider = createTauriModelWorkflowProvider({ core: { invoke: async (command, args) => {
      calls.push({ command, args });
      return JSON.stringify({ steps: [
        { task: "inspect_controls", arguments: {} },
        { task: "save_message", arguments: { message: "exact literal ✓" } },
        { task: invalid ? "undeclared_task" : "check_controls", arguments: {} },
      ] });
    } } }, { epoch: "workflow-page" });
    const session = new AgentWorkflowSession({ provider, host: {
      describe: async () => catalog,
      execute: async (plan) => { executed.push(plan.task); return { task: plan.task }; },
      verify: async (report) => { verified.push(report.task); return true; },
    } });
    const result = await session.submit("检查、保存、核对");
    assert.equal(calls.length, 1);
    assert.equal(calls[0].command, "agent_model_complete");
    assert.equal(calls[0].args.epoch, "workflow-page");
    if (invalid) {
      assert.equal(result.status, "failed");
      assert.equal(result.possiblePartialEffects, false);
      assert.deepEqual(executed, []);
    } else {
      assert.equal(result.status, "verified");
      assert.deepEqual(executed, ["inspect_controls", "save_message", "check_controls"]);
      assert.deepEqual(verified, executed);
      assert.equal(result.steps.length, 3);
    }
  }
});
