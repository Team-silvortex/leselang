import assert from "node:assert/strict";
import test from "node:test";
import { describeWorkflowOutcome } from "../dist/agent/workflow-display.js";

const plan = { steps: ["Inspect", "Save", "Check"].map((summary) => ({ summary, source: "source", arguments: {} })) };
const step = (status, type, value) => ({ taskId: 1, status, possiblePartialEffects: status !== "verified",
  ...(type ? { report: JSON.stringify({ result: { type, value } }) } : {}) });

test("workflow display retains each scalar result and does not claim simultaneous final postconditions", () => {
  const display = describeWorkflowOutcome({ taskId: 1, status: "verified", possiblePartialEffects: false, plan,
    steps: [step("verified", "integer", 3), step("verified", "string", "saved"), step("verified", "none", null)] });
  assert.match(display.summary, /共 3 个步骤，已验证 3/u);
  assert.match(display.outcome, /均已在执行后通过应用核对/u);
  assert.match(display.steps[0], /integer：3/u);
  assert.match(display.steps[1], /string："saved"/u);
  assert.match(display.steps[2], /none/u);
  assert.equal(display.reports.split("\n\n").length, 3);
});

test("a failed step's scalar is evidence only and the remaining step is displayed as unstarted", () => {
  const report = step("failed", "string", "saved");
  const display = describeWorkflowOutcome({ taskId: 2, status: "failed", possiblePartialEffects: true, plan,
    error: "application_verification_failed", steps: [step("verified", "integer", 3), report], stoppedStep: 1 });
  assert.match(display.outcome, /已验证 1\/3/u);
  assert.match(display.outcome, /已发生的操作会保留/u);
  assert.match(display.outcome, /后续步骤已停止/u);
  assert.doesNotMatch(display.steps[1], /已验证|saved/u);
  assert.match(display.steps[2], /未开始/u);
  assert.ok(display.reports.includes(report.report), "failed raw evidence is retained");
});

test("early refusal and cancellation cannot display execution or an internal cancellation code", () => {
  const early = describeWorkflowOutcome({ taskId: 3, status: "failed", possiblePartialEffects: false,
    error: "invalid_model_selection", steps: [] });
  assert.match(early.outcome, /应用操作未开始/u);
  assert.doesNotMatch(early.outcome, /0\/0/u);
  assert.equal(early.steps.length, 0);
  assert.equal(early.reports, "");
  const cancelled = describeWorkflowOutcome({ taskId: 4, status: "cancelled", possiblePartialEffects: true,
    error: "agent_task_cancelled", plan, steps: [step("verified", "integer", 3)], stoppedStep: 1 });
  assert.match(cancelled.outcome, /工作流已取消/u);
  assert.doesNotMatch(cancelled.outcome, /agent_task_cancelled/u);
  assert.match(cancelled.steps[1], /未开始/u);
  assert.match(cancelled.steps[2], /未开始/u);
});

test("result references and actual resolved inputs remain visible in separate evidence", () => {
  const template = { ...plan, bindings: [{ step: 1, argument: "message", fromStep: 0, format: "decimal" }] };
  const executed = { ...step("verified", "string", "saved"), plan: { ...plan.steps[1], arguments: { message: "18446744073709551615" } } };
  const display = describeWorkflowOutcome({ taskId: 5, status: "verified", possiblePartialEffects: false,
    plan: template, steps: [step("verified", "integer", 3), executed, step("verified", "none", null)] });
  assert.match(display.steps[1], /使用步骤 1 的已验证结果/u);
  assert.match(display.reports, /输入参数：\{"message":"18446744073709551615"\}/u);
  assert.ok(display.reports.includes(executed.report));
});

test("binding failures show actionable text without displaying internal validation codes", () => {
  for (const error of ["invalid_workflow_binding_target", "duplicate_workflow_binding", "invalid_workflow_result", "workflow_result_unverified"]) {
    const display = describeWorkflowOutcome({ taskId: 6, status: "failed", possiblePartialEffects: false, error, steps: [] });
    assert.doesNotMatch(display.outcome, /invalid_workflow|duplicate_workflow|workflow_result/u);
    assert.match(display.outcome, /输入|验证/u);
  }
});

test("control replacement during execution explains the stop and retains partial effects", () => {
  const display = describeWorkflowOutcome({ taskId: 15, status: "failed", possiblePartialEffects: true,
    error: "application_bindings_changed", plan, stoppedStep: 1,
    steps: [step("verified", "integer", 3), { taskId: 2, status: "failed", possiblePartialEffects: true,
      error: "application_bindings_changed" }] });
  assert.match(display.outcome, /应用控件已重新挂载或移除.*请重新观察当前应用/u);
  assert.match(display.outcome, /已发生的操作会保留.*不会自动重试/u);
  assert.match(display.steps[1], /未完成，后续步骤已停止/u);
  assert.match(display.steps[2], /未开始/u);
  assert.doesNotMatch(display.outcome, /application_bindings_changed/u);
  assert.doesNotMatch(display.reports, /验证诊断/u, "a stopped execution cannot invent a verification report");
});

const requirement = { step: 1, fromStep: 0, format: "string", equals: "expected ✓" };
const requiredPlan = { ...plan, requirements: [requirement] };

test("satisfied requirements show the declared condition and the actual verified result separately", () => {
  const checked = { ...requirement, actual: requirement.equals, satisfied: true };
  const display = describeWorkflowOutcome({ taskId: 7, status: "verified", possiblePartialEffects: false,
    plan: requiredPlan, requirements: [checked],
    steps: [step("verified", "string", requirement.equals), step("verified", "string", "saved"), step("verified", "none", null)] });
  assert.match(display.steps[1], /要求步骤 1 的已验证结果（文本）等于 "expected ✓"/u);
  assert.match(display.steps[1], /实际比较值 "expected ✓"，条件已满足/u);
  assert.match(display.steps[1], /已验证（string："saved"）/u);
  assert.match(display.outcome, /3 个步骤均已在执行后通过应用核对/u);
  assert.ok(display.reports.includes(JSON.stringify(checked)));
});

test("a mismatched requirement displays the target as unexecuted without inventing a report", () => {
  const checked = { ...requirement, actual: "different", satisfied: false };
  const display = describeWorkflowOutcome({ taskId: 8, status: "failed", possiblePartialEffects: true,
    plan: requiredPlan, requirements: [checked], error: "workflow_requirement_not_met", stoppedStep: 1,
    steps: [step("verified", "string", "different"), { taskId: 2, status: "failed", possiblePartialEffects: false,
      error: "workflow_requirement_not_met" }] });
  assert.match(display.steps[0], /已验证/u);
  assert.match(display.steps[1], /实际比较值 "different"，条件不满足/u);
  assert.match(display.steps[1], /本步骤未执行，后续步骤已停止/u);
  assert.doesNotMatch(display.steps[1], /已验证（|saved/u);
  assert.match(display.steps[2], /未开始/u);
  assert.match(display.outcome, /目标步骤未执行/u);
  assert.match(display.outcome, /已验证 1\/3/u);
  assert.match(display.outcome, /已发生的操作会保留/u);
  assert.doesNotMatch(display.outcome, /workflow_requirement_not_met|已完成：/u);
  assert.equal(display.reports.split("\n\n").length, 2, "one read report and one comparison, no target execution report");
  assert.ok(display.reports.includes(JSON.stringify(checked)));
});

test("unreached requirements stay unevaluated when earlier verification fails or is cancelled", () => {
  for (const status of ["failed", "cancelled"]) {
    const display = describeWorkflowOutcome({ taskId: 9, status, possiblePartialEffects: true,
      plan: requiredPlan, steps: [step(status, "string", "expected ✓")], stoppedStep: 0 });
    assert.match(display.steps[1], /要求步骤 1 的已验证结果/u);
    assert.match(display.steps[1], /未开始/u);
    assert.doesNotMatch(display.steps[1], /实际比较值|条件已满足|条件不满足/u);
    assert.doesNotMatch(display.reports, /前置条件核对/u);
  }
});

test("requirement admission errors display a local explanation before execution", () => {
  for (const error of ["invalid_workflow_requirements", "invalid_workflow_requirement_format", "duplicate_workflow_requirement"]) {
    const display = describeWorkflowOutcome({ taskId: 10, status: "failed", possiblePartialEffects: false, error, steps: [] });
    assert.match(display.outcome, /步骤前置条件不符合应用声明，未开始执行/u);
    assert.doesNotMatch(display.outcome, /invalid_workflow|duplicate_workflow/u);
    assert.equal(display.reports, "");
  }
});

const diagnosticOutcome = (verificationFailure) => ({ taskId: 11, status: "failed", possiblePartialEffects: true,
  plan, error: "application_verification_failed", stoppedStep: 1, verificationFailure,
  steps: [step("verified", "integer", 3), { ...step("failed", "string", "saved"), verificationFailure }] });

test("verification causes explain both the failed step and overall result without upgrading reported values", () => {
  const explanations = [
    ["invalid_report", "执行报告无法读取"],
    ["report_identity_mismatch", "执行报告与本次任务不对应"],
    ["execution_failed", "宿主报告本次执行失败"],
    ["execution_object_unavailable", "目标控件当前不存在或尚未挂载"],
    ["execution_fuel_exhausted", "计算预算已耗尽"],
    ["execution_deadline_exceeded", "本次执行超过宿主时限"],
    ["execution_scope_closed", "应用连接已关闭"],
    ["execution_cancelled", "宿主报告本次执行已取消"],
    ["report_budget_mismatch", "资源限制与应用声明不一致"],
    ["host_verification_missing", "未包含宿主核对结果"],
    ["result_mismatch", "返回结果不符合应用声明"],
    ["result_evidence_invalid", "返回结果缺少有效的报告证据"],
    ["result_evidence_mismatch", "返回结果与报告证据不一致"],
    ["completion_unavailable", "未取得本次任务的完成凭据"],
    ["plan_mismatch", "完成凭据与本次执行计划不对应"],
    ["report_mismatch", "完成凭据与执行报告不对应"],
    ["observation_invalid", "应用状态记录无效"],
    ["before_observation_mismatch", "执行前的应用状态与报告记录不一致"],
    ["after_observation_mismatch", "执行后再次读取的应用状态与报告记录不一致"],
    ["application_bindings_changed", "应用控件已重新挂载或移除"],
    ["verification_interrupted", "应用核对被中断"],
    ["invalid_verification_result", "宿主未返回有效的核对结论"],
  ];
  for (const [code, explanation] of explanations) {
    const diagnostic = { code };
    const result = diagnosticOutcome(diagnostic);
    const display = describeWorkflowOutcome(result);
    assert.ok(display.outcome.includes(explanation), code);
    assert.ok(display.steps[1].includes(explanation), code);
    assert.ok(!display.outcome.includes(code), "internal codes only belong in evidence");
    assert.ok(!display.steps[1].includes(code));
    assert.match(display.outcome, /已验证 1\/3.*已发生的操作会保留.*不会自动重试/u);
    assert.match(display.steps[1], /未完成，后续步骤已停止/u);
    assert.doesNotMatch(display.steps[1], /已验证|saved/u);
    assert.doesNotMatch(display.outcome, /已完成：|saved/u);
    assert.match(display.steps[2], /未开始/u);
    assert.ok(display.reports.includes(result.steps[1].report));
    assert.ok(display.reports.includes(`验证诊断：${JSON.stringify(diagnostic)}`));
  }
});

test("execution failure presentation preserves raw evidence without interpreting error text", () => {
  const verificationFailure = { code: "execution_object_unavailable" };
  const raw = JSON.stringify({ schema_version: 1, kind: "tauri_task_run", profile: "tauri.article",
    status: "failed", error: "arbitrary private failure text", possible_partial_effects: true,
    execution_failure: verificationFailure });
  const result = diagnosticOutcome(verificationFailure);
  result.steps[1].report = raw;
  const display = describeWorkflowOutcome(result);
  assert.match(display.outcome, /应用执行未完成.*目标控件当前不存在或尚未挂载/u);
  assert.doesNotMatch(display.outcome, /资源限制|应用结果未通过核对|arbitrary private failure text/u);
  assert.doesNotMatch(display.steps[1], /arbitrary private failure text|已验证/u);
  assert.match(display.steps[2], /未开始/u);
  assert.ok(display.reports.includes(raw), "original error belongs only in the raw evidence");

  const remoteCancelled = describeWorkflowOutcome(diagnosticOutcome({ code: "execution_cancelled" }));
  assert.match(remoteCancelled.outcome, /工作流未完成.*宿主报告本次执行已取消/u);
  assert.doesNotMatch(remoteCancelled.outcome, /工作流已取消/u, "a report cannot assert local cancellation");
});

test("declared check failures name only the predicate and retain a separate diagnostic record", () => {
  for (const code of ["predicate_failed", "predicate_evidence_invalid"]) {
    const diagnostic = { code, predicate: "content_saved" };
    const display = describeWorkflowOutcome(diagnosticOutcome(diagnostic));
    assert.match(display.steps[1], /应用声明的检查“content_saved”/u);
    assert.match(display.outcome, /应用声明的检查“content_saved”/u);
    assert.ok(display.reports.includes(JSON.stringify(diagnostic)));
    assert.doesNotMatch(display.steps[1], /predicate_|expected|actual|已验证/u);
  }
});

test("a diagnostic without a returned report never invents an execution result", () => {
  const diagnostic = { code: "completion_unavailable" };
  const display = describeWorkflowOutcome({ taskId: 12, status: "failed", possiblePartialEffects: true,
    plan, steps: [{ taskId: 1, status: "failed", possiblePartialEffects: true, verificationFailure: diagnostic }] });
  assert.match(display.outcome, /未取得本次任务的完成凭据/u, "step diagnostic explains overall failure when not duplicated at top level");
  assert.equal(display.reports, `步骤 1 · failed\n验证诊断：${JSON.stringify(diagnostic)}`);
  assert.doesNotMatch(display.reports, /输入参数|result/u);
  const overall = describeWorkflowOutcome({ taskId: 13, status: "failed", possiblePartialEffects: false,
    steps: [], verificationFailure: diagnostic });
  assert.match(overall.outcome, /未取得本次任务的完成凭据/u);
  assert.equal(overall.reports, `工作流 · 验证诊断\n${JSON.stringify(diagnostic)}`);
});

test("malformed or unknown diagnostics remain failures and do not display untrusted claims", () => {
  let getterReads = 0;
  const accessor = { get code() { getterReads++; throw new Error("untrusted"); } };
  const malformed = [false, true, null, [], "verified", {}, { code: false }, { code: "unknown_code" },
    { code: "__proto__" }, { code: "predicate_failed" }, { code: "predicate_failed", predicate: "已验证！" },
    { code: "predicate_failed", predicate: "true" }, { code: "predicate_failed", predicate: "_lese_internal" },
    { code: "predicate_failed", predicate: "x".repeat(65) }, { code: "report_mismatch", predicate: "content_saved" },
    { code: "predicate_failed", predicate: "content_saved", expected: "secret_expected", actual: "secret_actual" }, accessor];
  for (const diagnostic of malformed) {
    const display = describeWorkflowOutcome(diagnosticOutcome(diagnostic));
    assert.match(display.outcome, /工作流未完成.*宿主未返回有效的核对结论/u);
    assert.match(display.steps[1], /未完成.*宿主未返回有效的核对结论/u);
    assert.match(display.steps[2], /未开始/u);
    assert.doesNotMatch(display.outcome, /已完成：|unknown_code|secret_|已验证！|__proto__/u);
    assert.doesNotMatch(display.steps[1], /已验证|unknown_code|secret_|__proto__/u);
    assert.doesNotMatch(display.reports, /验证诊断|unknown_code|secret_|__proto__/u);
  }
  assert.equal(getterReads, 0, "display must not invoke a diagnostic accessor");
});

test("verified and cancelled outcomes never present stray diagnostics as failures", () => {
  const verificationFailure = { code: "predicate_failed", predicate: "content_saved" };
  for (const status of ["verified", "cancelled"]) {
    const clean = { taskId: 14, status, possiblePartialEffects: status === "cancelled", plan,
      steps: status === "verified" ? [step(status, "integer", 3), step(status, "string", "saved"), step(status, "none", null)]
        : [step(status, "string", "saved")] };
    const stray = { ...clean, verificationFailure, steps: clean.steps.map((value) => ({ ...value, verificationFailure })) };
    assert.deepEqual(describeWorkflowOutcome(stray), describeWorkflowOutcome(clean));
  }
});

test("legacy verification failures retain their exact display and unknown error text", () => {
  const result = diagnosticOutcome(undefined);
  delete result.verificationFailure;
  delete result.steps[1].verificationFailure;
  const display = describeWorkflowOutcome(result);
  assert.deepEqual(display, {
    summary: "本次共 3 个步骤，已验证 1 个。",
    outcome: "工作流未完成。 应用结果未通过核对，后续步骤已停止。 已验证 1/3 个步骤。 已发生的操作会保留，请查看应用当前状态；不会自动重试。",
    steps: ["步骤 1 · Inspect — 已验证（integer：3）。", "步骤 2 · Save — 未完成，后续步骤已停止。", "步骤 3 · Check — 未开始。"],
    reports: `步骤 1 · verified\n输入参数：{}\n${result.steps[0].report}\n\n步骤 2 · failed\n输入参数：{}\n${result.steps[1].report}`,
  });
  const unknown = describeWorkflowOutcome({ ...result, error: "legacy_host_failure" });
  assert.match(unknown.outcome, /legacy_host_failure/u);
});
