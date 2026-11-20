import type { AgentWorkflowOutcome } from "./bridge.js";
import { describeVerifiedResult } from "./result-display.js";

const messages: Readonly<Record<string, string>> = {
  model_transport_failed: "模型请求未完成，请检查宿主的模型配置或服务状态。",
  model_request_refused: "请求无法明确对应到应用声明的任务，请补充具体要求。",
  invalid_model_response: "模型没有返回有效的任务计划。",
  invalid_model_selection: "模型选择的任务或参数不符合应用声明。",
  invalid_model_message: "任务内容为空、过长或包含不支持的字符。",
  workflow_catalog_changed: "应用能力目录发生变化，剩余步骤已停止。",
  invalid_workflow_plan: "工作流不符合应用声明，未开始执行。",
  application_verification_failed: "应用结果未通过核对，后续步骤已停止。",
};

/** Presentation only. A returned scalar never upgrades a failed step's status. */
export function describeWorkflowOutcome(result: AgentWorkflowOutcome): {
  summary: string; outcome: string; steps: readonly string[]; reports: string;
} {
  const total = result.plan?.steps.length ?? 0;
  const completed = result.steps.filter((step) => step.status === "verified").length;
  const steps = result.plan?.steps.map((plan, index) => {
    const step = result.steps[index];
    const prefix = `步骤 ${index + 1} · ${plan.summary}`;
    if (!step) return `${prefix} — 未开始。`;
    if (step.status === "verified") return `${prefix} — 已验证（${describeVerifiedResult(step.report)}）。`;
    return `${prefix} — ${step.status === "cancelled" ? "已取消" : "未完成"}，后续步骤已停止。`;
  }) ?? [];
  const reason = result.error && result.status !== "cancelled" ? ` ${messages[result.error] ?? result.error}` : "";
  const outcome = result.status === "verified"
    ? `已完成：${completed} 个步骤均已在执行后通过应用核对。`
    : (result.status === "cancelled" ? "工作流已取消。" : "工作流未完成。") + reason
      + (total ? ` 已验证 ${completed}/${total} 个步骤。` : "")
      + (result.possiblePartialEffects ? " 已发生的操作会保留，请查看应用当前状态；不会自动重试。" : " 应用操作未开始。");
  const reports = result.steps.flatMap((step, index) => typeof step.report === "string"
    ? [`步骤 ${index + 1} · ${step.status}\n${step.report}`] : []).join("\n\n");
  return { summary: total ? `本次共 ${total} 个步骤，已验证 ${completed} 个。` : "未生成可执行工作流。", outcome, steps, reports };
}
