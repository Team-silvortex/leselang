import { AgentSession, prepareSemanticCatalog, type AgentState } from "./bridge.js";
import { createFormView } from "./form-view.js";
import { offlineProvider } from "./offline-provider.js";
import { createAgentHost } from "./agent-host.js";
import { describeVerifiedResult } from "./result-display.js";

function element<T extends HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error("application element missing");
  return node as T;
}
const form = createFormView({
  input: element<HTMLTextAreaElement>("message"),
  save: element<HTMLButtonElement>("save"), other: element<HTMLButtonElement>("other"),
  saved: element<HTMLPreElement>("saved-value"),
  saveCount: element<HTMLSpanElement>("save-count"), otherCount: element<HTMLSpanElement>("other-count"),
  status: element<HTMLParagraphElement>("status"),
});
const composer = element<HTMLFormElement>("task-form");
const message = element<HTMLTextAreaElement>("task-message");
const submit = element<HTMLButtonElement>("task-submit");
const cancel = element<HTMLButtonElement>("task-cancel");
const status = element<HTMLParagraphElement>("task-status");
const conversation = element<HTMLOListElement>("conversation");
const details = element<HTMLDetailsElement>("workflow-details");
const source = element<HTMLPreElement>("workflow-source");
const reportDetails = element<HTMLDetailsElement>("report-details");
const reportValue = element<HTMLPreElement>("report-value");
const tasks = element<HTMLUListElement>("available-tasks");
const epoch = crypto.randomUUID();
let currentRow: HTMLLIElement | null = null;
function update(state: AgentState): void {
  const labels: Record<AgentState["phase"], string> = {
    idle: "准备就绪。", describing: "正在读取应用能力…", planning: "正在准备工作流…",
    executing: "正在操作应用…", verifying: "正在核对应用结果…",
    cancelling: "正在取消，等待当前任务结束…", verified: "任务完成，应用结果已核对。",
    failed: "任务未完成。", cancelled: "任务已取消。",
  };
  status.textContent = labels[state.phase];
  if (state.summary && currentRow) {
    const summary = currentRow.querySelector(".plan-summary");
    if (summary) summary.textContent = state.summary;
  }
  cancel.disabled = !["describing", "planning", "executing", "verifying"].includes(state.phase);
}

async function connect(): Promise<void> {
  const api = window.__TAURI__;
  if (!api) throw new Error("应用助手需要在 Tauri 宿主内运行。");
  // A description is read again for every submitted task. This first read only
  // establishes that the page can reach its application-owned host.
  const catalog = prepareSemanticCatalog(await api.core.invoke("agent_describe", { raw: true, taskCatalog: true }));
  for (const task of catalog.tasks) {
    const item = document.createElement("li");
    const command = task.inputName === undefined ? `task ${task.id}` : `task ${task.id}:文本`;
    item.textContent = `${command} — ${task.summary}`;
    tasks.append(item);
  }
  const host = createAgentHost(api, form, {
    epoch,
    onSource(value) { details.hidden = false; source.textContent = value; },
  });
  const session = new AgentSession({ provider: offlineProvider, host, onEvent: update });
  cancel.addEventListener("click", () => { session.cancel(); });
  composer.addEventListener("submit", (event) => {
    event.preventDefault();
    if (session.busy) return;
    const input = message.value;
    const row = document.createElement("li");
    const request = document.createElement("p");
    request.className = "user-request";
    request.textContent = input;
    const summary = document.createElement("p");
    summary.className = "plan-summary";
    summary.textContent = "正在准备…";
    const outcome = document.createElement("p");
    outcome.className = "task-outcome";
    row.append(request, summary, outcome);
    conversation.append(row);
    // The display retains a bounded number of turns; it is not provider memory.
    while (conversation.children.length > 12) conversation.firstElementChild?.remove();
    currentRow = row;
    submit.disabled = true;
    message.disabled = true;
    details.hidden = true;
    source.textContent = "";
    reportDetails.hidden = true;
    reportValue.textContent = "";
    void session.submit(input).then((result) => {
      row.dataset.outcome = result.status;
      if (!result.plan) summary.textContent = "未生成工作流。";
      if (typeof result.report === "string") {
        reportDetails.hidden = false;
        reportValue.textContent = result.report;
      }
      if (result.status === "verified") {
        outcome.textContent = `已完成：宿主声明的任务结果已验证。返回结果（${describeVerifiedResult(result.report)}）。`;
      } else {
        const reason = result.error ? ` ${result.error}` : "";
        outcome.textContent = (result.status === "cancelled" ? "任务已取消。" : "任务未完成。") + reason
          + (result.possiblePartialEffects ? " 已发生的操作会保留，请查看应用当前状态。" : " 应用操作未开始。");
      }
    }).catch((error: unknown) => {
      row.dataset.outcome = "failed";
      outcome.textContent = error instanceof Error ? error.message : "任务无法开始。";
    }).finally(() => {
      submit.disabled = false;
      message.disabled = false;
      cancel.disabled = true;
      currentRow = null;
    });
  });
  window.addEventListener("pagehide", () => {
    session.cancel();
    host.close();
  }, { once: true });
  submit.disabled = false;
  status.textContent = "准备就绪。每条指令单独执行，结果会在应用中显示。";
}

submit.disabled = true;
void connect().catch((error: unknown) => {
  status.textContent = error instanceof Error ? error.message : "无法连接应用助手。";
  status.dataset.failed = "true";
});
