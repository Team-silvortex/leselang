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
