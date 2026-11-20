import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  createModelProvider, createModelWorkflowProvider, defineRegisteredTask, defineRegisteredTaskV3,
  defineSemanticCatalog, prepareSemanticCatalog, prepareSemanticWorkflow } from "../dist/index.js";

const lit = (kind, value) => ({ kind: "literal", value: { kind, value } });
const input = (name) => ({ kind: "input", name });
const path = (name) => [{ kind: "field", name }];
const predicate = (id, name) => ({ id, left: { kind: "scalar", scalar_type: "string", path: path(name) },
  relation: { kind: "equal" }, right: input(name) });
const signal = () => new AbortController().signal;
function definition(names = ["title", "content"]) {
  return {
    id: "save", summary: "Save the requested title and content independently.",
    inputs: names.map((name) => ({ name, scalar_type: "string" })), execution: "named_strings",
    steps: [{ kind: "call", binding: "saved", operation: "sample.save", arguments: [
      { name: "title", value: input("title") }, { name: "content", value: input("content") },
    ] }], result: { kind: "binding", name: "saved" },
    verification: { kind: "multi_run", max_fuel: 10_000, require_verified: false,
      result: { kind: "boolean", value: true }, predicates: [predicate("saved_title", "title"), predicate("saved_content", "content")] },
  };
}
function readTask(id) {
  return { id, summary: `Read ${id} from actual state.`, inputs: [], execution: "no_text",
    steps: [{ kind: "call", binding: "value", operation: `sample.${id}`, arguments: [] }],
    result: { kind: "binding", name: "value" },
    verification: { kind: "multi_run", max_fuel: 10_000, require_verified: false,
      result: { kind: "observed", scalar_type: "string", path: path("observed") },
      predicates: [{ id: "fresh", left: { kind: "scalar", scalar_type: "boolean", path: path("fresh") },
        relation: { kind: "equal" }, right: lit("boolean", true) }] },
  };
}
const base = {
  schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "multi.test", native_types: [],
  operations: [
    { name: "sample.save", required_capability: "sample.write", parameters: ["title", "content"].map((name) => ({
      name, required: true, types: [{ kind: "scalar", name: "string" }],
    })), result: { kind: "scalar", name: "boolean" } },
    ...["read_title", "read_content"].map((id) => ({ name: `sample.${id}`, required_capability: "sample.read",
      parameters: [], result: { kind: "scalar", name: "string" } })),
  ],
};
function catalog(task = definition(), version = 3) {
  return defineSemanticCatalog(base, { version,
    operations: base.operations.map(({ name }) => ({ operation: name, summary: name,
      effect: name === "sample.save" ? "write" : "read", completion: "value" })),
    tasks: [task, readTask("read_title"), readTask("read_content")],
  });
}
function report(plan, overrides = {}) {
  return { schema_version: 1, kind: "multi_run", profile: "multi.test", effects: 1, fuel_remaining: 9000,
    result: plan.task === "save" ? { type: "boolean", value: true } : { type: "string", value: plan.task === "read_title" ? "Observed title" : "Observed content" },
    ...(plan.task === "save" ? plan.arguments : { observed: plan.task === "read_title" ? "Observed title" : "Observed content", fresh: true }), ...overrides };
}

test("v3 names every string input, generates sorted parameters and verifies each exact argument", () => {
  const compiled = prepareSemanticCatalog(catalog());
  assert.deepEqual(compiled.tasks[0], { id: "save", summary: definition().summary, inputNames: ["title", "content"] });
  const args = { title: 'Title "✓"', content: 'line one\n{"source":"literal"}' };
  const prepared = compiled.prepare("save", args);
  assert.equal(prepared.plan.inputMode, "named_strings");
  assert.ok(prepared.plan.source.includes("fn main(content: string, title: string)"));
  assert.equal(prepared.plan.source.includes(args.content), false);
  const evidence = report(prepared.plan);
  assert.equal(prepared.verify(evidence), true);
  assert.equal(prepared.verify({ ...evidence, title: args.content, content: args.title }), false);
  args.title = "caller changed";
  assert.equal(prepared.plan.arguments.title, 'Title "✓"');
  assert.ok(Object.isFrozen(prepared.plan.arguments));
  assert.equal(compiled.prepare("read_title", {}).plan.inputMode, "named_strings");
  assert.deepEqual(compiled.tasks[1].inputNames, []);
});

test("v1/v2 retain their strict legacy shapes while v3 requires its execution convention", () => {
  for (const version of [1, 2]) assert.throws(() => catalog(definition(), version));
  for (const execution of ["text", "named_text", "no_text"]) {
    assert.throws(() => catalog({ ...definition(), execution }));
  }
  const legacy = readTask("save");
  legacy.steps = [{ kind: "call", binding: "value", operation: "sample.read_title", arguments: [] }];
  const old = prepareSemanticCatalog(catalog(legacy, 2));
  assert.equal(old.tasks[0].inputNames, undefined);
  assert.equal(old.prepare("save", {}).plan.inputMode, undefined);
  assert.throws(() => catalog(definition(), 4));
});

test("v3 rejects missing, extra, duplicate, nonstring and malformed inputs without coercion", () => {
  const compiled = prepareSemanticCatalog(catalog());
  for (const args of [{}, { title: "x" }, { title: "x", content: "y", extra: "z" }, { title: 1, content: "y" },
    { title: "x", content: true }, { title: "x", content: "\ud800" }, { title: "x", content: "\0" },
    { title: "x", content: "x".repeat(4097) }]) assert.throws(() => compiled.prepare("save", args));
  let reads = 0;
  assert.throws(() => compiled.prepare("save", { title: "x", get content() { reads++; return "y"; } }));
  assert.equal(reads, 0);
  for (const names of [["title", "title"], ["title", "content", "_lese_hidden"], Array.from({ length: 9 }, (_, i) => `p${i}`)]) {
    assert.throws(() => catalog(definition(names)));
  }
  const typed = definition(); typed.inputs[1].scalar_type = "boolean";
  assert.throws(() => catalog(typed));
  const unknown = definition(); unknown.verification.predicates[1].right.name = "missing";
  assert.throws(() => catalog(unknown));
});

test("eight v3 names are supported within per-value and aggregate byte bounds", () => {
  const names = ["title", "content", "a", "b", "c", "d", "e", "f"];
  const compiled = prepareSemanticCatalog(catalog(definition(names)));
  const args = Object.fromEntries(names.map((name) => [name, ""]));
  for (const name of names.slice(0, 4)) args[name] = "x".repeat(4096);
  assert.equal(Object.keys(compiled.prepare("save", args).plan.arguments).length, 8);
  args.c = "x";
  assert.throws(() => compiled.prepare("save", args), /total bound/);
  args.title = "界".repeat(1366);
  assert.throws(() => compiled.prepare("save", args));
});

test("offline provider parses named JSON only for v3 and rejects duplicate keys and coercions", async () => {
  const provider = createSemanticProvider();
  const context = (message, raw = catalog()) => ({ taskId: 1, message, catalog: raw });
  const plan = await provider.plan(context('task save:{"title":"标题","content":"literal\\nline"}'), signal());
  assert.deepEqual({ ...plan.arguments }, { title: "标题", content: "literal\nline" });
  assert.equal(plan.inputMode, "named_strings");
  for (const message of ['task save:text', 'task save:{"title":"a","title":"b","content":"x"}',
    'task save:{"title":"a","content":3}', 'task save:{"title":"a"}', 'task save', 'task read_title:{}']) {
    await assert.rejects(provider.plan(context(message), signal()));
  }
  assert.equal((await provider.plan(context("task read_title"), signal())).inputMode, "named_strings");
  const legacyTask = definition(["title"]); legacyTask.execution = "text";
  legacyTask.steps[0].arguments[1].value = lit("string", "fixed");
  legacyTask.verification.predicates = [predicate("saved_title", "title")];
  const legacy = await provider.plan(context('task save:{"content":"literal JSON"}', catalog(legacyTask, 2)), signal());
  assert.equal(legacy.arguments.title, '{"content":"literal JSON"}');
  assert.equal(legacy.inputMode, undefined);
});

test("model selection uses declared v3 names, with no model-selected input mode or source", async () => {
  let request;
  const provider = createModelProvider({ async complete(value) {
    request = value;
    return '{"task":"save","arguments":{"content":" B ","title":"T"}}';
  } });
  const plan = await provider.plan({ taskId: 1, message: "Save title T and content B", catalog: catalog() }, signal());
  assert.match(request.system, /one to eight named string values/);
  assert.doesNotMatch(request.system, /explicitly enables resultBindings/);
  assert.deepEqual(JSON.parse(request.user).tasks[0].inputNames, ["title", "content"]);
  assert.deepEqual({ ...plan.arguments }, { title: "T", content: " B " });
  assert.equal(plan.inputMode, "named_strings");
  for (const response of ['{"task":"save","arguments":{"title":"x"}}',
    '{"task":"save","arguments":{"title":"x","content":"y"},"inputMode":"named_strings"}',
    '{"task":"save","arguments":{"title":"x","content":"y","content":"z"}}']) {
    const invalid = createModelProvider({ async complete() { return response; } });
    await assert.rejects(invalid.plan({ taskId: 1, message: "save", catalog: catalog() }, signal()));
  }
});

test("session admission requires the explicit mode and owns all named values", async () => {
  const prepared = prepareSemanticCatalog(catalog()).prepare("save", { title: "T", content: "B" }).plan;
  let executions = 0;
  const host = { async describe() { return catalog(); }, async execute(plan) { executions++; return report(plan); }, verify() { return true; } };
  for (const plan of [{ ...prepared, inputMode: undefined }, { ...prepared, inputMode: "anything" }]) {
    const session = new AgentSession({ host, provider: { async plan() { return plan; } } });
    assert.equal((await session.submit("save")).status, "failed");
  }
  assert.equal(executions, 0);
  const session = new AgentSession({ host, provider: { async plan() { return prepared; } } });
  assert.equal((await session.submit("save")).status, "verified");
  assert.equal(executions, 1);
});

test("registered host rejects stripped or forged mode before observing or attaching the application", async () => {
  const raw = catalog(); let touches = 0;
  const host = createRegisteredAgentHost({ window: "multi-mode-test", epoch: "case-1",
    application: { attach() { touches++; }, detach() {}, observe() { touches++; return []; } },
    transport: { async describe() { return raw; }, async execute() { throw new Error("must not execute"); }, reply() {}, cancel() {}, close() {} },
    observations: { before: "before", after: "after" } });
  try {
    await host.describe(signal());
    // A no-input plan is shape-valid without the marker, so admission must also
    // compare it with the host-prepared plan instead of only counting arguments.
    const plan = prepareSemanticCatalog(raw).prepare("read_title", {}).plan;
    const { inputMode, ...stripped } = plan;
    await assert.rejects(host.execute(stripped, signal()), /plan_mismatch/);
    assert.equal(touches, 0);
  } finally { host.close(); }
});

test("workflow binds two independently verified results to their named target arguments", async () => {
  const raw = catalog();
  const selection = [
    { task: "read_title", arguments: {} }, { task: "read_content", arguments: {} },
    { task: "save", arguments: { title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" } } },
  ];
  const prepared = prepareSemanticWorkflow(raw, selection);
  assert.equal(prepared.bindings.length, 2);
  const seen = [];
  const session = new AgentWorkflowSession({ provider: { async plan() { return prepared; } }, host: {
    async describe() { return raw; }, async execute(plan) { seen.push(plan); return report(plan); },
    verify(value, plan) { return prepareSemanticCatalog(raw).prepare(plan.task, plan.arguments).verify(value); },
  } });
  const outcome = await session.submit("read both values, then save them");
  assert.equal(outcome.status, "verified", outcome.error);
  assert.deepEqual({ ...seen[2].arguments }, { title: "Observed title", content: "Observed content" });
  assert.ok(seen.every((plan) => plan.inputMode === "named_strings"));
  const model = createModelWorkflowProvider({ resultBindings: true, async complete(request) {
    assert.match(request.system, /explicitly enables resultBindings/);
    assert.match(request.system, /String bounds apply to its resolved value/);
    return JSON.stringify({ steps: selection });
  } });
  const selected = await model.plan({ taskId: 1, message: "Read both and save", catalog: raw }, signal());
  assert.equal(selected.bindings.length, 2);
});

test("workflow rejects missing mode, duplicate targets and future references before execution", async () => {
  const raw = catalog(), compiled = prepareSemanticCatalog(raw);
  let calls = 0;
  const plan = compiled.prepare("read_title", {}).plan;
  const { inputMode, ...stripped } = plan;
  const session = new AgentWorkflowSession({ provider: { async plan() { return { steps: [stripped] }; } }, host: {
    async describe() { return raw; }, async execute() { calls++; }, verify() { return true; },
  } });
  assert.equal((await session.submit("read")).status, "failed");
  assert.equal(calls, 0);
  assert.throws(() => prepareSemanticWorkflow(raw, [
    { task: "read_title", arguments: {} },
    { task: "save", arguments: { title: { fromStep: 1, format: "string" }, content: "literal" } },
  ]));
});

test("v3 workflow refuses a later write when either referenced result was not verified", async () => {
  const raw = catalog(), seen = [];
  const session = new AgentWorkflowSession({ provider: { async plan() { return prepareSemanticWorkflow(raw, [
    { task: "read_title", arguments: {} }, { task: "read_content", arguments: {} },
    { task: "save", arguments: { title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" } } },
  ]); } }, host: {
    async describe() { return raw; }, async execute(plan) { seen.push(plan.task); return report(plan); },
    verify(value, plan) { return plan.task !== "read_content"; },
  } });
  const outcome = await session.submit("read then save");
  assert.equal(outcome.status, "failed");
  assert.deepEqual(seen, ["read_title", "read_content"]);
});

test("v3 workflow binding bounds permit several fields per step, never arbitrary extra targets", () => {
  const names = ["title", "content", "a", "b", "c", "d", "e", "f"];
  const raw = catalog(definition(names));
  const plan = prepareSemanticWorkflow(raw, [
    { task: "read_title", arguments: {} },
    ...Array.from({ length: 7 }, () => ({ task: "save", arguments: Object.fromEntries(names.map((name) => [name, { fromStep: 0, format: "string" }])) })),
  ]);
  assert.equal(plan.bindings.length, 56);
  assert.ok(plan.steps.every((step) => step.inputMode === "named_strings"));
});

test("registered v3 constructor handles two input macros without relaxing the original constructor", async () => {
  const native = JSON.parse(await readFile(new URL("../../../fixtures/host-semantics/tauri-agent-semantics-v2.json", import.meta.url), "utf8"));
  const { semantics, ...nativeBase } = native;
  const task = { ...definition(), failure: { kind: "boolean", value: false },
    steps: [
      { kind: "set_control_text", control: "title", binding: "title_after", text: input("title") },
      { kind: "set_control_text", control: "content", binding: "content_after", text: input("content") },
    ], result: lit("boolean", true) };
  assert.throws(() => defineRegisteredTask(task));
  const expanded = defineRegisteredTaskV3(task);
  assert.ok(Object.isFrozen(expanded.inputs));
  const raw = defineSemanticCatalog(nativeBase, { version: 3, operations: semantics.operations, tasks: [expanded] });
  const prepared = prepareSemanticCatalog(raw).prepare("save", { title: "T", content: "B" });
  assert.equal(prepared.expectedEffects, 8);
  assert.equal(prepared.plan.inputMode, "named_strings");
  assert.ok(prepared.plan.source.includes("content: string, title: string"));
  assert.throws(() => defineRegisteredTaskV3({ ...task, inputs: [...task.inputs, task.inputs[0]] }));

  const publish = defineRegisteredTaskV3({ ...task, steps: [...task.steps,
    { kind: "click_control", control: "publish", binding: "published" }] });
  const publishCatalog = defineSemanticCatalog(nativeBase, { version: 3, operations: semantics.operations, tasks: [publish] });
  const published = prepareSemanticCatalog(publishCatalog).prepare("save", { title: "T", content: "B" });
  assert.equal(published.expectedEffects, 12);
  // The host allows sixteen expanded bindings. Splitting inside these macros
  // would retain five helper arguments and exceed it; their minimal live cuts
  // need only the second input (two entry inputs + twelve calls + one capture).
  assert.match(published.plan.source, /fn _lese_stage_1\(content: string\)/);
  assert.match(published.plan.source, /fn _lese_stage_2\(\)/);
  assert.equal((published.plan.source.match(/= /gu) ?? []).length, 3);
});

test("v3 partition ties prefer fewer helpers and the longest earlier chunk", () => {
  const task = { ...definition(), inputs: [], execution: "no_text", result: lit("boolean", true),
    steps: Array.from({ length: 12 }, (_, index) => ({ kind: "call", binding: `value_${index}`,
      operation: "sample.save", arguments: [{ name: "title", value: lit("string", "T") },
        { name: "content", value: lit("string", "B") }] })),
    verification: { ...definition().verification, predicates: [
      { id: "fresh", left: lit("boolean", true), relation: { kind: "equal" }, right: lit("boolean", true) },
    ] } };
  const source = prepareSemanticCatalog(catalog(task)).prepare("save", {}).plan.source;
  const legacy = prepareSemanticCatalog(catalog(task, 2)).prepare("save", {}).plan.source;
  assert.equal(source, legacy, "equal-cost partitions preserve the longest prefix convention");
  const functions = source.trim().split("\n").slice(1);
  assert.equal(functions.length, 2);
  assert.equal((functions[0].match(/bind\(/gu) ?? []).length, 9);
  assert.equal((functions[1].match(/bind\(/gu) ?? []).length, 3);
});

test("a v3 no-input task keeps its explicit mode through registered transport dispatch", async () => {
  const raw = catalog(), prepared = prepareSemanticCatalog(raw).prepare("read_title", {});
  let run;
  const evidence = JSON.stringify({ ...report(prepared.plan), before: [], after: [] });
  const host = createRegisteredAgentHost({ window: "multi-mode-forward", epoch: "case-1",
    application: { attach() {}, detach() {}, observe() { return []; } },
    transport: { async describe() { return raw; }, async execute(value) { run = value; return evidence; }, reply() {}, cancel() {}, close() {} },
    observations: { before: "before", after: "after" } });
  try {
    await host.describe(signal());
    assert.equal(await host.execute(prepared.plan, signal()), evidence);
    assert.equal(run.inputMode, "named_strings");
    assert.deepEqual(Object.keys(run.arguments), []);
    assert.equal(host.verify(evidence, prepared.plan, signal()), true);
  } finally { host.close(); }
});

test("binding resolution rechecks the total input budget before a later task enters execution", async () => {
  const names = ["title", "content", "a", "b", "c"];
  const raw = catalog(definition(names)), executed = [];
  const session = new AgentWorkflowSession({ provider: { async plan() { return prepareSemanticWorkflow(raw, [
    { task: "read_title", arguments: {} },
    { task: "save", arguments: Object.fromEntries(names.map((name) => [name, { fromStep: 0, format: "string" }])) },
  ]); } }, host: {
    async describe() { return raw; }, async execute(plan) {
      executed.push(plan.task);
      return report(plan, { result: { type: "string", value: "x".repeat(4096) }, observed: "x".repeat(4096) });
    },
    verify() { return true; },
  } });
  const outcome = await session.submit("reuse the observed value");
  assert.equal(outcome.status, "failed");
  assert.match(outcome.error, /total bound/);
  assert.deepEqual(executed, ["read_title"]);
});
