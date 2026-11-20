import { AgentWorkflowSession, prepareSemanticCatalog, type AgentWorkflowState } from "./bridge.js";
import { createFormView } from "./form-view.js";
import { offlineProvider } from "./offline-provider.js";
import { createAgentHost } from "./agent-host.js";
import { describeWorkflowOutcome } from "./workflow-display.js";
import { createTauriModelWorkflowProvider, modelInfo } from "./model-provider.js";

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
const providerMode = element<HTMLSelectElement>("provider-mode");
const providerLabel = element<HTMLSpanElement>("provider-label");
const providerNote = element<HTMLParagraphElement>("provider-note");
const epoch = crypto.randomUUID();
let sources: string[] = [];
let currentRow: HTMLLIElement | null = null;
function update(state: AgentWorkflowState): void {
  const labels: Record<AgentWorkflowState["phase"], string> = {
    idle: "准备就绪。", describing: "正在读取应用能力…", planning: "正在准备工作流…",
    executing: "正在操作应用…", verifying: "正在核对应用结果…",
    cancelling: "正在取消，等待当前任务结束…", verified: "任务完成，应用结果已核对。",
    failed: "任务未完成。", cancelled: "任务已取消。",
  };
  const progress = state.totalSteps ? ` 已验证 ${state.completedSteps}/${state.totalSteps} 个步骤。` : "";
  status.textContent = labels[state.phase] + progress;
  if (state.summary && currentRow) {
    const summary = currentRow.querySelector(".plan-summary");
    if (summary) summary.textContent = `${state.stepIndex === undefined ? "" : `步骤 ${state.stepIndex + 1}/${state.totalSteps} · `}${state.summary}`;
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
    onSource(value) {
      sources.push(value);
      details.hidden = false;
      source.textContent = sources.map((program, index) => `步骤 ${index + 1}\n${program}`).join("\n\n");
    },
  });
  const modelProvider = createTauriModelWorkflowProvider(api, { epoch });
  let modelName: string | undefined;
  let configurationError = false;
  try {
    const info = await modelInfo(api);
    if (info.configured) modelName = info.model;
  } catch { configurationError = true; }
  element<HTMLOptionElement>("model-option").disabled = modelName === undefined;
  providerMode.value = modelName ? "model" : "offline";
  const showProvider = () => {
    const usingModel = providerMode.value === "model" && modelName !== undefined;
    providerLabel.textContent = usingModel ? `模型 · ${modelName}` : "离线参考规划器";
    providerNote.textContent = usingModel
      ? "可描述最多 8 个有序步骤，例如：先检查控件，再保存指定文本，最后核对。每步通过应用验证后才继续；本次指令与应用能力目录会发送给已配置的模型服务。"
      : (configurationError ? "模型配置不可用。" : modelName ? "" : "尚未配置模型服务。")
        + "离线模式使用下方任务命令，也支持“保存：文本”。每条指令独立执行。";
    message.placeholder = usingModel ? "先检查控件，再保存“周五完成设计评审”，最后核对控件" : "task fill_and_save:周五完成设计评审";
  };
  showProvider();
  providerMode.addEventListener("change", showProvider);
  const session = new AgentWorkflowSession({
    provider: { async plan(context, signal) {
      return providerMode.value === "model" ? modelProvider.plan(context, signal)
        : { steps: [await offlineProvider.plan(context, signal)] };
    } },
    host, onEvent: update,
  });
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
    const stepResults = document.createElement("ol");
    stepResults.className = "workflow-steps";
    row.append(request, summary, stepResults, outcome);
    conversation.append(row);
    // The display retains a bounded number of turns; it is not provider memory.
    while (conversation.children.length > 12) conversation.firstElementChild?.remove();
    currentRow = row;
    submit.disabled = true;
    providerMode.disabled = true;
    message.disabled = true;
    details.hidden = true;
    sources = [];
    source.textContent = "";
    reportDetails.hidden = true;
    reportValue.textContent = "";
    void session.submit(input).then((result) => {
      row.dataset.outcome = result.status;
      const described = describeWorkflowOutcome(result);
      summary.textContent = described.summary;
      outcome.textContent = described.outcome;
      for (const text of described.steps) {
        const step = document.createElement("li");
        step.textContent = text;
        stepResults.append(step);
      }
      if (described.reports) {
        reportDetails.hidden = false;
        reportValue.textContent = described.reports;
      }
    }).catch((error: unknown) => {
      row.dataset.outcome = "failed";
      outcome.textContent = error instanceof Error ? error.message : "任务无法开始。";
    }).finally(() => {
      submit.disabled = false;
      providerMode.disabled = false;
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
  providerMode.disabled = false;
  status.textContent = "准备就绪。每条指令单独执行，结果会在应用中显示。";
}

submit.disabled = true;
providerMode.disabled = true;
void connect().catch((error: unknown) => {
  status.textContent = error instanceof Error ? error.message : "无法连接应用助手。";
  status.dataset.failed = "true";
});
