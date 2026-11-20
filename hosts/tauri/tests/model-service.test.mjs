// No server, network, model inference or native execution: feed the labelled
// protocol fixture through the same planner used by the Tauri conversation.
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { prepareSemanticCatalog } from "../frontend/dist/bridge.js";
import { createTauriModelWorkflowProvider } from "../frontend/dist/agent/model-provider.js";
import { selectProtocolFixture } from "./model-service.mjs";

const article = await readFile(new URL("../../../fixtures/host-semantics/tauri-article-semantics-v3.json", import.meta.url), "utf8");
const catalog = prepareSemanticCatalog(article);
const literalMessage = "发布标题“文章协议回归 ✓”和正文“两个独立输入分别传入。”，再读取标题和正文并核对文章。";
const boundMessage = "读取当前文章标题和正文，将两个已验证结果分别用于发布，再核对文章。";

function bodyFor(context, model = "protocol-workflow-fixture-not-a-model") {
  return {
    model, stream: false, response_format: { type: "json_object" },
    messages: [{ role: "system", content: "Explicit protocol fixture, not a model." },
      { role: "user", content: JSON.stringify(context) }],
  };
}

async function plan(message) {
  const calls = [];
  const provider = createTauriModelWorkflowProvider({ core: { invoke: async (command, args) => {
    calls.push({ command, args });
    assert.equal(command, "agent_model_complete");
    const context = JSON.parse(args.request.user);
    assert.equal(JSON.parse(context.catalog).profile, "tauri.article");
    assert.deepEqual(context.tasks.map(({ id, inputNames }) => ({ id, inputNames })), [
      { id: "read_title", inputNames: [] }, { id: "read_content", inputNames: [] },
      { id: "publish_article", inputNames: ["title", "content"] }, { id: "check_article", inputNames: [] },
    ]);
    const body = bodyFor(context);
    body.messages[0].content = args.request.system;
    const { fixture, workflow } = selectProtocolFixture(body);
    assert.equal(workflow, true);
    return JSON.stringify(fixture.selection);
  } } }, { epoch: "article-protocol-fixture" });
  const result = await provider.plan({ taskId: 1, message, catalog: article }, new AbortController().signal);
  assert.equal(calls.length, 1);
  assert.equal(result.steps.every((step) => step.inputMode === "named_strings"), true);
  assert.equal(result.steps.reduce((effects, step) => effects
    + catalog.prepare(step.task, step.arguments).expectedEffects, 0), 17);
  return result;
}

test("Article literal fixture selects separate title and content with the shared model workflow planner", async () => {
  const result = await plan(literalMessage);
  assert.deepEqual(result.steps.map((step) => step.task), ["publish_article", "read_title", "read_content", "check_article"]);
  assert.deepEqual({ ...result.steps[0].arguments }, { title: "文章协议回归 ✓", content: "两个独立输入分别传入。" });
  assert.equal(result.bindings, undefined);
  assert.doesNotMatch(result.steps[0].source, /文章协议回归|两个独立输入/u);
});

test("Article observed fixture binds both earlier string results independently and declares seventeen Effects", async () => {
  const result = await plan(boundMessage);
  assert.deepEqual(result.steps.map((step) => step.task), ["read_title", "read_content", "publish_article", "check_article"]);
  assert.deepEqual(result.bindings, [
    { step: 2, argument: "title", fromStep: 0, format: "string" },
    { step: 2, argument: "content", fromStep: 1, format: "string" },
  ]);
  assert.deepEqual({ ...result.steps[2].arguments }, { title: "", content: "" });
  assert.equal(Object.isFrozen(result.bindings), true);
});

test("Article fixture refuses foreign profiles, legacy modes and inconsistent named-input declarations", () => {
  const context = () => ({ message: literalMessage, tasks: structuredClone(catalog.tasks), catalog: article });
  for (const mutate of [
    (value, raw) => { raw.profile = "tauri.typescript"; },
    (value, raw) => { raw.profile = "tauri.taskboard"; },
    (value, raw) => { raw.semantics.version = 2; },
    (value) => { delete value.tasks[0].inputNames; },
    (value) => { value.tasks[2].inputName = "title"; },
    (value) => { value.tasks[2].inputNames = ["title"]; },
    (value) => { value.tasks[2].inputNames = ["content", "title"]; },
    (value) => { value.tasks[3] = value.tasks[0]; },
    (value, raw) => { raw.semantics.tasks[2].inputs[1].name = "body"; },
    (value, raw) => { raw.semantics.tasks[2].inputs[1].scalar_type = "boolean"; },
    (value, raw) => { raw.semantics.tasks[2].execution = "no_text"; },
    (value, raw) => { raw.semantics.tasks.pop(); },
  ]) {
    const value = context(), raw = JSON.parse(article);
    mutate(value, raw);
    value.catalog = JSON.stringify(raw);
    assert.throws(() => selectProtocolFixture(bodyFor(value)), /fixture request mismatch/u);
  }
  for (const model of ["protocol-fixture-not-a-model", "an-external-model"]) {
    assert.throws(() => selectProtocolFixture(bodyFor(context(), model)), /fixture request mismatch/u);
  }
});

test("existing form and board fixture replies retain their selections, failures and delays", async () => {
  const form = await readFile(new URL("../../../fixtures/host-semantics/tauri-agent-semantics-v2.json", import.meta.url), "utf8");
  const board = await readFile(new URL("../../../fixtures/host-semantics/tauri-board-semantics-v2.json", import.meta.url), "utf8");
  const select = (raw, message, model) => selectProtocolFixture(bodyFor({ message,
    tasks: prepareSemanticCatalog(raw).tasks, catalog: raw }, model)).fixture;
  assert.deepEqual(select(form, "请把“模型接入回归 ✓”保存到应用里。", "protocol-fixture-not-a-model").selection,
    { task: "save_message", arguments: { message: "模型接入回归 ✓" } });
  assert.equal(select(form, "测试模型服务限流").status, 429);
  assert.equal(select(form, "测试取消模型请求").delay, 15000);
  const boardPlan = select(board, "读取报告任务状态，只有它还是 pending 才完成它，再读取状态并核对任务板。").selection.steps;
  assert.deepEqual(boardPlan.map((step) => step.task), ["read_review_status", "complete_review", "read_review_status", "check_board"]);
  assert.deepEqual(boardPlan[1].requires, { fromStep: 0, format: "string", equals: "pending" });
  assert.equal(select(board, "测试任务板引用表单任务").selection.steps[1].task, "save_message");
  assert.equal(select(board, "测试取消任务板规划").delay, 15000);
});
