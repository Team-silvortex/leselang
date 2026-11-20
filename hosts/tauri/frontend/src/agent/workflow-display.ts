import type { AgentWorkflowOutcome, VerificationDiagnostic } from "../bridge.js";
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

const verificationMessages: Readonly<Record<VerificationDiagnostic["code"], string>> = {
  invalid_report: "执行报告无法读取，请查看原始报告。",
  report_identity_mismatch: "执行报告与本次任务不对应，请核对任务来源。",
  report_budget_mismatch: "执行报告的资源限制与应用声明不一致，请核对宿主配置。",
  host_verification_missing: "执行报告未包含宿主核对结果，请检查宿主的验证配置。",
  result_mismatch: "返回结果不符合应用声明，请查看原始报告。",
  result_evidence_invalid: "返回结果缺少有效的报告证据，请查看原始报告。",
  result_evidence_mismatch: "返回结果与报告证据不一致，请查看原始报告。",
  predicate_evidence_invalid: "应用声明的检查缺少有效的报告证据，请查看原始报告。",
  predicate_failed: "应用声明的检查未通过，请查看当前应用状态。",
  completion_unavailable: "未取得本次任务的完成凭据，请查看当前应用状态。",
  plan_mismatch: "完成凭据与本次执行计划不对应，请核对任务来源。",
  report_mismatch: "完成凭据与执行报告不对应，请查看原始报告。",
  observation_invalid: "应用状态记录无效，请检查宿主的状态读取。",
  before_observation_mismatch: "执行前的应用状态与报告记录不一致，请查看原始报告。",
  after_observation_mismatch: "执行后再次读取的应用状态与报告记录不一致，请查看当前应用状态。",
  verification_interrupted: "应用核对被中断，请查看当前应用状态。",
  invalid_verification_result: "宿主未返回有效的核对结论，请检查宿主的验证配置。",
};

/** Only diagnostic codes and declared predicate identifiers belong in this UI. */
function describeVerificationFailure(value: unknown): { text: string; raw?: string } | undefined {
  if (value === undefined) return undefined;
  const invalid = { text: verificationMessages.invalid_verification_result };
  try {
    if (!value || typeof value !== "object" || Array.isArray(value)
      || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) return invalid;
    const fields = Object.getOwnPropertyDescriptors(value);
    if (Reflect.ownKeys(fields).some((key) => key !== "code" && key !== "predicate")
      || !fields.code || !("value" in fields.code) || !fields.code.enumerable) return invalid;
    const code: unknown = fields.code.value;
    if (typeof code !== "string" || !Object.hasOwn(verificationMessages, code)) return invalid;
    const predicateCode = code === "predicate_failed" || code === "predicate_evidence_invalid";
    const predicate: unknown = fields.predicate?.value;
    if (predicateCode ? !fields.predicate || !("value" in fields.predicate) || !fields.predicate.enumerable
      || typeof predicate !== "string" || !/^[A-Za-z_][A-Za-z_0-9]{0,63}$/u.test(predicate)
      || ["fn", "true", "false", "none", "body"].includes(predicate) || predicate.startsWith("_lese_")
      : fields.predicate !== undefined) return invalid;
    const text = verificationMessages[code as VerificationDiagnostic["code"]];
    return {
      text: predicateCode ? text.replace("应用声明的检查", `应用声明的检查“${predicate}”`) : text,
      raw: JSON.stringify({ code, ...(predicateCode ? { predicate } : {}) }),
    };
  } catch { return invalid; }
}

/** Presentation only. A returned scalar never upgrades a failed step's status. */
export function describeWorkflowOutcome(result: AgentWorkflowOutcome): {
  summary: string; outcome: string; steps: readonly string[]; reports: string;
} {
  const total = result.plan?.steps.length ?? 0;
  const completed = result.steps.filter((step) => step.status === "verified").length;
  const failures = result.steps.map((step) => result.status === "failed" && step.status === "failed"
    ? describeVerificationFailure(step.verificationFailure) : undefined);
  const verificationFailure = result.status === "failed"
    ? describeVerificationFailure(result.verificationFailure) ?? failures.find((failure) => failure !== undefined)
    : undefined;
  const steps = result.plan?.steps.map((plan, index) => {
    const step = result.steps[index];
    const failure = failures[index];
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
    return `${prefix} — ${step.status === "cancelled" ? "已取消" : "未完成"}，后续步骤已停止。${failure ? ` ${failure.text}` : ""}`;
  }) ?? [];
  const bindingError = result.error?.startsWith("invalid_workflow_binding") || result.error === "duplicate_workflow_binding";
  const requirementError = result.error?.startsWith("invalid_workflow_requirement") || result.error === "duplicate_workflow_requirement";
  const reason = result.error && result.status !== "cancelled"
    ? ` ${bindingError ? "步骤之间的输入关系不符合应用声明，未开始执行。"
      : requirementError ? "步骤前置条件不符合应用声明，未开始执行。" : messages[result.error] ?? result.error}` : "";
  const outcome = result.status === "verified"
    ? `已完成：${completed} 个步骤均已在执行后通过应用核对。`
    : (result.status === "cancelled" ? "工作流已取消。" : "工作流未完成。") + reason
      + (verificationFailure ? ` ${verificationFailure.text}` : "")
      + (total ? ` 已验证 ${completed}/${total} 个步骤。` : "")
      + (result.possiblePartialEffects ? " 已发生的操作会保留，请查看应用当前状态；不会自动重试。" : " 应用操作未开始。");
  const reports = [
    ...result.steps.flatMap((step, index) => {
      const failure = failures[index];
      const evidence = [
        ...(typeof step.report === "string" ? [`输入参数：${JSON.stringify(step.plan?.arguments ?? {})}\n${step.report}`] : []),
        ...(failure?.raw ? [`验证诊断：${failure.raw}`] : []),
      ];
      return evidence.length ? [`步骤 ${index + 1} · ${step.status}\n${evidence.join("\n")}`] : [];
    }),
    ...(verificationFailure?.raw && !failures.some((failure) => failure?.raw === verificationFailure.raw)
      ? [`工作流 · 验证诊断\n${verificationFailure.raw}`] : []),
    ...(result.requirements?.map((requirement) => `步骤 ${requirement.step + 1} · 前置条件核对\n${JSON.stringify(requirement)}`) ?? []),
  ].join("\n\n");
  return { summary: total ? `本次共 ${total} 个步骤，已验证 ${completed} 个。` : "未生成可执行工作流。", outcome, steps, reports };
}
