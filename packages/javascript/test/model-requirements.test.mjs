import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { AgentWorkflowSession, createModelProvider, createModelWorkflowProvider, prepareSemanticWorkflow } from "@leselang/bridge";

const raw = await readFile(new URL("../../../fixtures/host-semantics/tauri-agent-semantics-v2.json", import.meta.url), "utf8");
const signal = () => new AbortController().signal;
const context = (catalog = raw) => ({ taskId: 1, message: "读取当前文本，只有等于预期值才保存，否则停止。", catalog });
const requirement = (format = "string", equals = "预期 ✓", fromStep = 0) => ({ fromStep, format, equals });
const step = (task, arguments_ = {}, requires) => ({ task, arguments: arguments_, ...(requires === undefined ? {} : { requires }) });
const guarded = (requires = requirement(), arguments_ = { message: "literal" }) => [
  step("read_message"), step("save_message", arguments_, requires), step("check_controls"),
];
const response = (steps) => JSON.stringify({ steps });
const provider = (reply, options = {}) => createModelWorkflowProvider({
  resultRequirements: true, ...options, async complete() { return reply; },
});

// The transports below are deterministic protocol fixtures, not model inference.
test("requirements are independently opt-in and old providers reject the extra field", async () => {
  const reply = response(guarded());
  for (const options of [{}, { resultBindings: true }, { resultRequirements: false }]) {
    await assert.rejects(createModelWorkflowProvider({ ...options, async complete() { return reply; } })
      .plan(context(), signal()), /invalid_model_response/u);
  }
  await assert.rejects(createModelProvider({ resultRequirements: true,
    async complete() { return JSON.stringify(step("save_message", { message: "x" }, requirement())); },
  }).plan(context(), signal()), /invalid_model_response/u);
  for (const resultRequirements of [null, "true", 1, {}, []]) {
    assert.throws(() => createModelWorkflowProvider({ resultRequirements, async complete() { return reply; } }), /invalid_model_options/u);
  }
  const plain = [step("read_message"), step("save_message", { message: "literal" })];
  assert.deepEqual(await provider(response(plain)).plan(context(), signal()), prepareSemanticWorkflow(raw, plain));
  assert.deepEqual(Object.keys(await provider(response(plain)).plan(context(), signal())), ["steps"]);
  const bound = guarded(requirement(), { message: { fromStep: 0, format: "string" } });
  await assert.rejects(provider(response(bound)).plan(context(), signal()), /invalid_model_response/u);
  const result = await provider(response(bound), { resultBindings: true }).plan(context(), signal());
  assert.deepEqual(result, prepareSemanticWorkflow(raw, bound));
  assert.equal(result.steps[1].arguments.message, "");
  assert.deepEqual(result.requirements, [{ step: 1, ...requirement() }]);
});

test("the enabled prompt describes exact conditions and does not weaken the host contract", async () => {
  for (const resultBindings of [false, true]) {
    let calls = 0;
    const selected = guarded();
    const p = createModelWorkflowProvider({ resultRequirements: true, resultBindings,
      async complete(request, forwarded) {
        calls++;
        assert.equal(forwarded, abort);
        assert.equal(Object.isFrozen(request), true);
        assert.equal(JSON.parse(request.user).catalog, raw);
        assert.match(request.system, /both semantic and application verification/u);
        assert.match(request.system, /stop the entire workflow before this step/u);
        assert.match(request.system, /do not lock application state/u);
        assert.match(request.system, /18446744073709551615/u);
        assert.match(request.system, /No arbitrary result paths/u);
        assert.match(request.system, /no conversation history/u);
        assert.equal(new TextEncoder().encode(request.system).length <= 16 * 1024, true);
        if (!resultBindings) assert.match(request.system, /arguments cannot contain result references/u);
        return response(selected);
      },
    });
    const abort = signal();
    const result = await p.plan(context(), abort);
    assert.deepEqual(result, prepareSemanticWorkflow(raw, selected));
    assert.equal(calls, 1);
    assert.ok(!result.steps[1].source.includes("预期 ✓"));
    assert.equal(Object.isFrozen(result.requirements[0]), true);
  }
});

test("requirements work with renamed task and operation declarations", async () => {
  const catalog = raw.replaceAll("tauri.typescript", "notes.demo").replaceAll("gui.", "notes.")
    .replaceAll('"read_message"', '"read_note"').replaceAll('"save_message"', '"publish_note"')
    .replaceAll('"message"', '"content"');
  const steps = [step("read_note"), step("publish_note", { content: "literal" }, requirement())];
  const result = await provider(response(steps)).plan(context(catalog), signal());
  assert.deepEqual(result, prepareSemanticWorkflow(catalog, steps));
  assert.match(result.steps[1].source, /notes\.set_text/u);
});

test("requirement JSON is strict and cannot smuggle source, evidence or alternate behavior", async () => {
  for (const requires of [null, [], {}, "x", { fromStep: 0 },
    { ...requirement(), equals: 7 }, { ...requirement(), equals: true },
    { ...requirement(), equals: "\0" }, { ...requirement(), equals: "\ud800" },
    { ...requirement(), equals: "中".repeat(1366) },
    { ...requirement(), fromStep: "0" }, { ...requirement(), fromStep: -1 },
    { ...requirement(), fromStep: 0.5 }, { ...requirement(), format: "auto" },
    { ...requirement(), operator: "not_equal" }, { ...requirement(), onFailure: "skip" },
    { ...requirement(), actual: "预期 ✓" }, { ...requirement(), satisfied: true },
    { ...requirement(), source: "fn main() = true" }, { ...requirement(), path: ["result"] },
  ]) {
    await assert.rejects(provider(response(guarded(requires))).plan(context(), signal()), /invalid_model_response/u);
  }
  for (const token of ["0.0", "0e0", "9007199254740993"]) {
    const reply = response(guarded()).replace('"fromStep":0', `"fromStep":${token}`);
    await assert.rejects(provider(reply).plan(context(), signal()), /invalid_model_response/u);
  }
  for (const reply of [
    response(guarded()).replace('"equals":"预期 ✓"', '"equals":"预期 ✓","equals":"forged"'),
    response(guarded()).replace('"requires":{', '"requires":{},"requires":{'),
    JSON.stringify({ steps: guarded(), requirements: [] }),
    response(guarded()).replace('"requires":', '"when":'),
    response(guarded()) + " trailing",
  ]) await assert.rejects(provider(reply).plan(context(), signal()), /invalid_model_response/u);
});

test("all graph and type conditions are checked before any earlier task executes", async () => {
  const badPlans = [
    guarded(requirement("decimal", "3")), guarded(requirement("string", "x", 1)),
    guarded(requirement("string", "x", 2)), guarded(requirement("string", "x", 7)),
    [step("check_controls"), step("save_message", { message: "x" }, requirement())],
    [step("inspect_controls"), step("save_message", { message: "x" }, requirement("decimal", "03"))],
    [step("inspect_controls"), step("save_message", { message: "x" }, requirement("decimal", "18446744073709551616"))],
    [step("fill_and_save", { text: "must not save" }), step("check_controls", {}, requirement("boolean", "True"))],
    [...guarded(), step("undeclared")],
  ];
  for (const steps of badPlans) {
    let executed = 0;
    const session = new AgentWorkflowSession({ provider: provider(response(steps)), host: {
      async describe() { return raw; }, async execute() { executed++; }, verify() { return true; },
    } });
    const outcome = await session.submit("test complete admission");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "invalid_model_selection");
    assert.equal(outcome.possiblePartialEffects, false);
    assert.equal(executed, 0);
    assert.equal(outcome.requirements, undefined);
  }
});

test("string content remains literal and typed requirement constants retain exact digits", async () => {
  for (const [first, format, values] of [
    [step("read_message"), "string", ["", "x".repeat(4096), '引号" \\ \n', '{"fromStep":1,"satisfied":true}']],
    [step("inspect_controls"), "decimal", ["0", "18446744073709551615"]],
    [step("fill_and_save", { text: "x" }), "boolean", ["true", "false"]],
  ]) {
    for (const equals of values) {
      const result = await provider(response([first, step("check_controls", {}, requirement(format, equals))]))
        .plan(context(), signal());
      assert.equal(result.requirements[0].equals, equals);
      assert.equal(result.requirements[0].format, format);
    }
  }
});

test("enabled planning keeps refusal, transport and reply bounds without fallback", async () => {
  await assert.rejects(provider('{"error":"private refused text"}').plan(context(), signal()), { message: "model_request_refused" });
  await assert.rejects(provider(response(Array.from({ length: 8 }, (_, index) => step("save_message", { message: "x".repeat(4096) },
    index ? requirement("string", "saved", index - 1) : undefined)))).plan(context(), signal()), /invalid_model_response/u);
  let calls = 0;
  const p = createModelWorkflowProvider({ resultRequirements: true, async complete() { calls++; throw new Error("Bearer private"); } });
  await assert.rejects(p.plan(context("{}"), signal()));
  assert.equal(calls, 0);
  await assert.rejects(p.plan(context(), signal()), { message: "model_transport_failed" });
  assert.equal(calls, 1);
});

test("late conditional plans cannot execute after cancellation", async () => {
  let release, started, executions = 0;
  const ready = new Promise((resolve) => { started = resolve; });
  const session = new AgentWorkflowSession({ provider: createModelWorkflowProvider({
    resultRequirements: true, resultBindings: true,
    async complete(_request, forwarded) { started(forwarded); return new Promise((resolve) => { release = resolve; }); },
  }), host: { async describe() { return raw; }, async execute() { executions++; }, verify() { return true; } } });
  const pending = session.submit("test conditional planning cancellation");
  const forwarded = await ready;
  assert.equal(session.cancel(), true);
  assert.equal(forwarded.aborted, true);
  assert.equal(session.busy, true);
  await assert.rejects(session.submit("overlap"), /agent_task_pending/u);
  release(response(guarded(requirement(), { message: { fromStep: 0, format: "string" } })));
  const result = await pending;
  assert.equal(result.status, "cancelled");
  assert.equal(result.requirements, undefined);
  assert.equal(result.possiblePartialEffects, false);
  assert.equal(executions, 0);
  assert.equal(session.busy, false);
});
