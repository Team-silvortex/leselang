import type { AgentWorkflowOutcome } from "../bridge.js";
import { describeVerifiedResult } from "./result-display.js";

const formats = { string: "文本", decimal: "十进制数", boolean: "布尔值" } as const;

const messages: Readonly<Record<string, string>> = {
  model_transport_failed: "模型请求未完成，请检查宿主的模型配置或服务状态。",
  model_request_refused: "请求无法明确对应到应用声明的任务，请补充具体要求。",
  invalid_model_response: "模型没有返回有效的任务计划。",
  invalid_model_selection: "模型选择的任务或参数不符合应用声明。",
  invalid_model_message: "任务内容为空、过长或包含不支持的字符。",
  workflow_catalog_changed: "应用能力目录发生变化，剩余步骤已停止。",
  invalid_workflow_plan: "工作流不符合应用声明，未开始执行。",
  invalid_workflow_result: "前序结果无法用作后续输入，剩余步骤已停止。",
  workflow_result_unverified: "前序结果尚未通过验证，剩余步骤已停止。",
  workflow_requirement_not_met: "前序已验证结果不符合本次指令的条件，目标步骤未执行，后续步骤已停止。",
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
    const bindings = result.plan?.bindings?.filter((binding) => binding.step === index) ?? [];
    const requirement = result.plan?.requirements?.find((requirement) => requirement.step === index);
    const checked = result.requirements?.find((requirement) => requirement.step === index);
    const inputs = [
      ...bindings.map((binding) => `使用步骤 ${binding.fromStep + 1} 的已验证结果`),
      ...(requirement ? [`要求步骤 ${requirement.fromStep + 1} 的已验证结果（${formats[requirement.format]}）等于 ${JSON.stringify(requirement.equals)}`] : []),
      ...(checked ? [`实际比较值 ${JSON.stringify(checked.actual)}，条件${checked.satisfied ? "已满足" : "不满足"}`] : []),
    ].join("；");
    const prefix = `步骤 ${index + 1} · ${plan.summary}${inputs ? `（${inputs}）` : ""}`;
    if (checked?.satisfied === false) return `${prefix} — 前置条件不满足，本步骤未执行，后续步骤已停止。`;
    if (!step) return `${prefix} — 未开始。`;
    if (step.status === "verified") return `${prefix} — 已验证（${describeVerifiedResult(step.report)}）。`;
    return `${prefix} — ${step.status === "cancelled" ? "已取消" : "未完成"}，后续步骤已停止。`;
  }) ?? [];
  const bindingError = result.error?.startsWith("invalid_workflow_binding") || result.error === "duplicate_workflow_binding";
  const requirementError = result.error?.startsWith("invalid_workflow_requirement") || result.error === "duplicate_workflow_requirement";
  const reason = result.error && result.status !== "cancelled"
    ? ` ${bindingError ? "步骤之间的输入关系不符合应用声明，未开始执行。"
      : requirementError ? "步骤前置条件不符合应用声明，未开始执行。" : messages[result.error] ?? result.error}` : "";
  const outcome = result.status === "verified"
    ? `已完成：${completed} 个步骤均已在执行后通过应用核对。`
    : (result.status === "cancelled" ? "工作流已取消。" : "工作流未完成。") + reason
      + (total ? ` 已验证 ${completed}/${total} 个步骤。` : "")
      + (result.possiblePartialEffects ? " 已发生的操作会保留，请查看应用当前状态；不会自动重试。" : " 应用操作未开始。");
  const reports = [
    ...result.steps.flatMap((step, index) => typeof step.report === "string"
      ? [`步骤 ${index + 1} · ${step.status}\n输入参数：${JSON.stringify(step.plan?.arguments ?? {})}\n${step.report}`] : []),
    ...(result.requirements?.map((requirement) => `步骤 ${requirement.step + 1} · 前置条件核对\n${JSON.stringify(requirement)}`) ?? []),
  ].join("\n\n");
  return { summary: total ? `本次共 ${total} 个步骤，已验证 ${completed} 个。` : "未生成可执行工作流。", outcome, steps, reports };
}
