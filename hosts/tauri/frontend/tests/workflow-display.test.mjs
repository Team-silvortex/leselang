import assert from "node:assert/strict";
import test from "node:test";
import { describeWorkflowOutcome } from "../dist/workflow-display.js";

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
