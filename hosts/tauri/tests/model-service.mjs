// Explicit protocol fixture, NOT a model. Never used by the application unless
// the developer deliberately points LESE_MODEL_ENDPOINT at its loopback URL.
// Run: node hosts/tauri/tests/model-service.mjs [port]
import http from "node:http";

const port = process.argv[2] === undefined ? 0 : Number(process.argv[2]);
if (!Number.isInteger(port) || port < 0 || port > 65535) throw new Error("invalid fixture port");
const examples = new Map([
  ["请把“模型接入回归 ✓”保存到应用里。", { selection: { task: "save_message", arguments: { message: "模型接入回归 ✓" } } }],
  ["看看这个应用有多少个控件，不要修改内容。", { selection: { task: "inspect_controls", arguments: {} } }],
  ["请帮我发送一封邮件。", { selection: { error: "The catalog does not declare an email task." } }],
  ["测试未声明任务", { selection: { task: "undeclared_task", arguments: {} } }],
  ["测试取消模型请求", { selection: { task: "save_message", arguments: { message: "这条不应保存" } }, delay: 15000 }],
  ["测试模型服务限流", { status: 429 }],
  ["先检查控件，再保存“多步骤工作流 ✓”，最后核对控件。", { selection: { steps: [
    { task: "inspect_controls", arguments: {} },
    { task: "save_message", arguments: { message: "多步骤工作流 ✓" } },
    { task: "check_controls", arguments: {} },
  ] } }],
  ["测试后续步骤无效", { selection: { steps: [
    { task: "save_message", arguments: { message: "不应开始第一步" } },
    { task: "undeclared_task", arguments: {} },
  ] } }],
  ["先检查控件数量，把这个数量保存为文本，再核对控件。", { selection: { steps: [
    { task: "inspect_controls", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "decimal" } } },
    { task: "check_controls", arguments: {} },
  ] } }],
  ["测试结果引用类型错误", { selection: { steps: [
    { task: "inspect_controls", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "string" } } },
  ] } }],
  ["读取当前输入框的文本，原样保存，再核对控件。", { selection: { steps: [
    { task: "read_message", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "string" } } },
    { task: "check_controls", arguments: {} },
  ] } }],
  ["测试动态观察结果类型错误", { selection: { steps: [
    { task: "read_message", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "decimal" } } },
  ] } }],
  ["读取当前文本，只有它等于“条件执行回归 ✓”时才原样保存，再核对控件。", { selection: { steps: [
    { task: "read_message", arguments: {} },
    { task: "save_message", arguments: { message: { fromStep: 0, format: "string" } },
      requires: { fromStep: 0, format: "string", equals: "条件执行回归 ✓" } },
    { task: "check_controls", arguments: {} },
  ] } }],
  ["测试条件结果类型错误", { selection: { steps: [
    { task: "read_message", arguments: {} },
    { task: "save_message", arguments: { message: "不应开始执行" },
      requires: { fromStep: 0, format: "decimal", equals: "3" } },
  ] } }],
  ["测试引用尚未完成的步骤", { selection: { steps: [
    { task: "save_message", arguments: { message: { fromStep: 1, format: "string" } } },
    { task: "save_message", arguments: { message: "不应开始执行" } },
  ] } }],
  ["读取报告任务状态，只有它还是 pending 才完成它，再读取状态并核对任务板。", { profile: "tauri.taskboard", selection: { steps: [
    { task: "read_review_status", arguments: {} },
    { task: "complete_review", arguments: {}, requires: { fromStep: 0, format: "string", equals: "pending" } },
    { task: "read_review_status", arguments: {} },
    { task: "check_board", arguments: {} },
  ] } }],
  ["测试任务板引用表单任务", { profile: "tauri.taskboard", selection: { steps: [
    { task: "complete_archive", arguments: {} },
    { task: "save_message", arguments: { message: "不应开始执行" } },
  ] } }],
  ["测试取消任务板规划", { profile: "tauri.taskboard", selection: { steps: [
    { task: "complete_archive", arguments: {} },
  ] }, delay: 15000 }],
]);
let count = 0;
const server = http.createServer(async (request, response) => {
  response.setHeader("Content-Type", "application/json");
  if (request.method !== "POST" || request.url !== "/v1/chat/completions") {
    response.writeHead(404).end("{}"); return;
  }
  let bytes = 0;
  const chunks = [];
  for await (const chunk of request) {
    bytes += chunk.length;
    if (bytes > 3 * 1024 * 1024) { response.writeHead(413).end("{}"); return; }
    chunks.push(chunk);
  }
  let fixture;
  let workflow = false;
  try {
    const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    workflow = body.model === "protocol-workflow-fixture-not-a-model";
    if ((!workflow && body.model !== "protocol-fixture-not-a-model") || body.stream !== false
      || body.response_format?.type !== "json_object" || body.messages?.length !== 2
      || body.messages[0].role !== "system" || body.messages[1].role !== "user") throw new Error();
    const context = JSON.parse(body.messages[1].content);
    const catalog = JSON.parse(context.catalog);
    if (!Array.isArray(context.tasks) || !catalog.semantics) throw new Error();
    fixture = examples.get(context.message);
    if (!fixture) throw new Error();
    if (fixture.profile && catalog.profile !== fixture.profile) throw new Error();
    if (!workflow && fixture.selection?.steps) throw new Error();
  } catch { response.writeHead(400).end('{"error":"fixture request mismatch"}'); return; }
  const id = ++count;
  console.log(JSON.stringify({ event: "fixture_request", id, status: fixture.status ?? 200, delayed: !!fixture.delay }));
  let timer;
  const reply = () => {
    if (response.destroyed) return;
    if (fixture.status) response.writeHead(fixture.status).end('{"error":"fixture service unavailable"}');
    else response.end(JSON.stringify({ choices: [{ index: 0, finish_reason: "stop", message: {
      role: "assistant", content: JSON.stringify(workflow && fixture.selection?.task
        ? { steps: [fixture.selection] } : fixture.selection),
    } }] }));
  };
  response.once("close", () => {
    clearTimeout(timer);
    console.log(JSON.stringify({ event: "fixture_closed", id, completed: response.writableFinished }));
  });
  if (fixture.delay) timer = setTimeout(reply, fixture.delay);
  else reply();
});
server.listen(port, "127.0.0.1", () => {
  console.log(JSON.stringify({ fixture: true, pid: process.pid, endpoint: `http://127.0.0.1:${server.address().port}/v1/chat/completions`,
    models: ["protocol-fixture-not-a-model", "protocol-workflow-fixture-not-a-model"] }));
});
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => server.close(() => process.exit(0)));
