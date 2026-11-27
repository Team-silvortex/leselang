import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", timeout: 30_000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

test("packed public ESM and declarations work in an external JavaScript/TypeScript consumer", async () => {
  const directory = await mkdtemp(join(tmpdir(), "leselang-bridge-package-"));
  try {
    const cache = join(directory, "npm-cache");
    const [packed] = JSON.parse(run("npm", [
      "pack", "--ignore-scripts", "--offline", "--json", "--pack-destination", directory, "--cache", cache,
    ], root));
    assert.deepEqual(packed.files.map((entry) => entry.path).sort(), [
      "LICENSE", "README.md",
      "dist/agent/data.d.ts", "dist/agent/data.js", "dist/agent/session.d.ts", "dist/agent/session.js",
      "dist/agent/registered-evidence.d.ts", "dist/agent/registered-evidence.js",
      "dist/agent/registered-host.d.ts", "dist/agent/registered-host.js",
      "dist/agent/registered-application.d.ts", "dist/agent/registered-application.js",
      "dist/agent/workflow-bindings.d.ts", "dist/agent/workflow-bindings.js", "dist/agent/workflow.d.ts", "dist/agent/workflow.js",
      "dist/bridge.d.ts", "dist/bridge.js",
      "dist/index.d.ts", "dist/index.js",
      "dist/dom/index.d.ts", "dist/dom/index.js", "dist/dom/controls.d.ts", "dist/dom/controls.js",
      "dist/verification.d.ts", "dist/verification.js",
      "dist/node/index.d.ts", "dist/node/index.js", "dist/node/transport.d.ts", "dist/node/transport.js",
      "dist/node/operation-host.d.ts", "dist/node/operation-host.js",
      "dist/operations/index.d.ts", "dist/operations/index.js",
      "dist/providers/model.d.ts", "dist/providers/model.js", "dist/providers/semantic.d.ts", "dist/providers/semantic.js",
      "dist/semantics/catalog.d.ts", "dist/semantics/catalog.js", "dist/semantics/index.d.ts", "dist/semantics/index.js",
      "dist/semantics/authoring.d.ts", "dist/semantics/authoring.js", "dist/semantics/declarations.d.ts", "dist/semantics/declarations.js",
      "dist/semantics/registered-task.d.ts", "dist/semantics/registered-task.js",
      "dist/semantics/json.d.ts", "dist/semantics/json.js",
      "dist/semantics/report.d.ts", "dist/semantics/report.js", "dist/semantics/source.d.ts", "dist/semantics/source.js",
      "package.json",
    ].sort());
    await writeFile(join(directory, "package.json"), JSON.stringify({ name: "external-bridge-consumer", private: true, type: "module" }));
    run("npm", [
      "install", join(directory, packed.filename), "--offline", "--ignore-scripts", "--no-audit", "--no-fund", "--cache", cache,
    ], directory);
    const installed = JSON.parse(await readFile(join(directory, "node_modules/@leselang/bridge/package.json"), "utf8"));
    assert.equal(installed.private, true);
    assert.equal(installed.license, "MIT");
    assert.deepEqual(installed.dependencies ?? {}, {});

    await writeFile(join(directory, "consumer.mjs"), `
import assert from "node:assert/strict";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createDomButtonControl, createDomTextControl } from "@leselang/bridge/dom";
import { Bridge, AgentSession, AgentWorkflowSession, createRegisteredApplication, createDynamicRegisteredApplication, defineSemanticCatalog, prepareSemanticCatalog, createSemanticProvider, createModelProvider, createModelWorkflowProvider, prepareSemanticWorkflow } from "@leselang/bridge";
assert.equal(typeof createNodeAgentTransport, "function");
assert.equal(typeof createDomButtonControl, "function");
assert.equal(typeof createDomTextControl, "function");
assert.equal(typeof globalThis.document, "undefined", "the optional DOM entry can be imported during server rendering");
assert.equal(Object.hasOwn(await import("@leselang/bridge"), "createDomButtonControl"), false);
await assert.rejects(import("@leselang/bridge/dist/dom/controls.js"), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
assert.throws(() => createNodeAgentTransport({ executable: "relative-worker", catalog: (base) => base }), /node_worker_invalid_options/);
assert.equal(typeof AgentWorkflowSession, "function");
assert.equal(typeof prepareSemanticWorkflow, "function");
assert.equal(typeof createModelWorkflowProvider({ async complete() { return '{}'; } }).plan, "function");
assert.equal(typeof prepareSemanticCatalog, "function");
assert.equal(typeof createSemanticProvider().plan, "function");
assert.equal(typeof createModelProvider({ async complete() { return '{}'; } }).plan, "function");
assert.throws(() => prepareSemanticCatalog('{}'));
const scope = { window: "node-main", epoch: "instance-1", session: "task-1" };
const bridge = new Bridge(scope);
const state = { role: "button", title: "Increment", value: "0", enabled: true, editable: false, clicks: 0 };
const application = createRegisteredApplication({ counter: {
  read: () => state,
  async click(context) {
    await Promise.resolve();
    context.commit(() => { state.clicks++; state.value = String(state.clicks); context.changed("counter"); });
  },
} });
application.attach(bridge);
let request = 0;
const call = (operation, arguments_ = {}) => bridge.handle({ transport_version: 1, ...scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ });
const target = (await call("gui.find", { name: "counter" })).value;
const before = (await call("gui.observe", { target })).value;
assert.equal((await call("gui.click", { target, expected_revision: before.revision })).status, "ok");
const after = (await call("gui.observe", { target })).value;
assert.equal(after.clicks, 1);
assert.equal(after.value, "1");
assert.equal(before.value, "0");
assert.equal(application.observe()[0].clicks, 1);
await assert.rejects(import("@leselang/bridge/src/index.js"), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
application.detach(bridge);
assert.equal(bridge.closed, true);
assert.equal((await call("gui.list")).status, "rejected");
const dynamic = createDynamicRegisteredApplication({ counter: null });
assert.deepEqual(dynamic.observe(), []);
assert.equal(dynamic.bindingRevision(), 0);
dynamic.mount("counter", { read: () => state });
const beforeReplacement = dynamic.observe();
dynamic.replace("counter", { read: () => ({ ...state }) });
assert.deepEqual(dynamic.observe(), beforeReplacement);
assert.equal(dynamic.bindingRevision(), 2);
dynamic.unmount("counter");
assert.deepEqual(dynamic.observe(), []);
assert.equal(Object.hasOwn(application, "bindingRevision"), false, "the old static factory keeps its shape");
const session = new AgentSession({
  provider: { async plan({ message }) { return { summary: "Save", source: "fn main(text: string) = true", arguments: { text: message } }; } },
  host: {
    async describe() { return { profile: "external" }; },
    async execute(plan) { return { saved: plan.arguments.text }; },
    verify(report, plan) { return report.saved === plan.arguments.text; },
  },
});
assert.equal((await session.submit("literal data")).status, "verified");
const observedCatalog = {
  schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "external.observed",
  native_types: [], operations: [{ name: "sample.read", required_capability: "sample", parameters: [], result: { kind: "scalar", name: "string" } }],
  semantics: { version: 2,
    operations: [{ operation: "sample.read", summary: "Read current text", effect: "read", completion: "value" }],
    tasks: [{ id: "read_current", summary: "Read current text", inputs: [], execution: "no_text",
      steps: [{ kind: "call", binding: "value", operation: "sample.read", arguments: [] }], result: { kind: "binding", name: "value" },
      verification: { kind: "external_report", max_fuel: 100, require_verified: false,
        result: { kind: "observed", scalar_type: "string", path: [{ kind: "field", name: "observed_value" }] },
        predicates: [{ id: "observed", left: { kind: "scalar", scalar_type: "boolean", path: [{ kind: "field", name: "fresh" }] }, relation: { kind: "equal" }, right: { kind: "literal", value: { kind: "boolean", value: true } } }],
      },
    }],
  },
};
const observed = prepareSemanticCatalog(observedCatalog).prepare("read_current", {});
const { semantics: observedDescription, ...observedBase } = observedCatalog;
const authored = defineSemanticCatalog(observedBase, observedDescription);
assert.equal(typeof authored, "string");
assert.deepEqual(prepareSemanticCatalog(authored).prepare("read_current", {}).plan, observed.plan);
for (const value of ["first value", "second value"]) {
  const report = { schema_version: 1, profile: "external.observed", kind: "external_report", effects: 1, fuel_remaining: 99,
    result: { type: "string", value }, observed_value: value, fresh: true };
  assert.equal(observed.verify(report), true);
  assert.deepEqual(observed.verifyDetailed(report), { verified: true });
  report.observed_value = "stale";
  assert.equal(observed.verify(report), false);
  const failure = observed.verifyDetailed(report);
  assert.deepEqual(failure, { verified: false, diagnostic: { code: "result_evidence_mismatch" } });
  assert.ok(Object.isFrozen(failure) && Object.isFrozen(failure.diagnostic));
}
const configured = prepareSemanticCatalog(observedCatalog, { executionReports: "v1" }).prepare("read_current", {});
assert.deepEqual(configured.plan, observed.plan);
const executionFailure = { schema_version: 1, profile: "external.observed", kind: "external_report",
  status: "failed", error: "Host object is unavailable", possible_partial_effects: true,
  execution_failure: { code: "execution_object_unavailable" } };
assert.deepEqual(configured.verifyDetailed(JSON.stringify(executionFailure)),
  { verified: false, diagnostic: { code: "execution_object_unavailable" } });
assert.deepEqual(observed.verifyDetailed(JSON.stringify(executionFailure)),
  { verified: false, diagnostic: { code: "report_budget_mismatch" } });
assert.equal(configured.verify(executionFailure), false);
for (const actual of ["expected", "changed"]) {
  let executions = 0;
  const guarded = new AgentWorkflowSession({
    provider: { async plan({ catalog }) { return prepareSemanticWorkflow(catalog, [
      { task: "read_current", arguments: {} },
      { task: "read_current", arguments: {}, requires: { fromStep: 0, format: "string", equals: "expected" } },
    ]); } },
    host: {
      async describe() { return observedCatalog; },
      async execute() { executions++; return { schema_version: 1, profile: "external.observed", kind: "external_report", effects: 1, fuel_remaining: 99,
        result: { type: "string", value: actual }, observed_value: actual, fresh: true }; },
      verify(report) { return observed.verify(report); },
    },
  });
  const outcome = await guarded.submit("continue only after the expected read");
  assert.equal(outcome.status, actual === "expected" ? "verified" : "failed");
  assert.equal(executions, actual === "expected" ? 2 : 1);
  assert.equal(outcome.requirements[0].actual, actual);
  assert.equal(outcome.requirements[0].satisfied, actual === "expected");
  if (actual !== "expected") assert.equal(outcome.error, "workflow_requirement_not_met");
}
`);
    run(process.execPath, ["consumer.mjs"], directory);

    // Separate installed-package integration: scripted transport exercises the
    // public registered host and real Bridge, not a language VM or Tauri mock.
    const board = await readFile(join(root, "../../fixtures/host-semantics/tauri-board-semantics-v2.json"), "utf8");
    await writeFile(join(directory, "catalog.json"), board
      .replaceAll('"tauri.taskboard"', '"external.registered"')
      .replaceAll('"tauri_task_run"', '"external_task_run"')
      .replaceAll('"native_before"', '"objects_before"')
      .replaceAll('"native_after"', '"objects_after"'));
    await writeFile(join(directory, "registered-consumer.mjs"), `
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { AgentSession, createRegisteredApplication, createRegisteredAgentHost, createSemanticProvider, defineRegisteredTask, defineSemanticCatalog, prepareSemanticCatalog } from "@leselang/bridge";
const { semantics, ...base } = JSON.parse(await readFile(new URL("./catalog.json", import.meta.url), "utf8"));
const catalog = defineSemanticCatalog(base, { ...semantics, tasks: semantics.tasks
  .filter((task) => task.id.startsWith("complete_"))
  .map((task) => defineRegisteredTask({ ...task, failure: { kind: "string", value: "failed" },
    steps: [{ kind: "click_control", control: task.id.slice("complete_".length), binding: "after" }],
  })) });
const prepared = prepareSemanticCatalog(catalog);
const rows = [
  { identifier: "review", role: "button", title: "核对示例报告", value: "pending", enabled: true, editable: false, clicks: 0 },
  { identifier: "archive", role: "button", title: "整理示例资料", value: "pending", enabled: true, editable: false, clicks: 0 },
];
let attached = null, deliver, starts = 0, observations = 0;
const scopes = [], sources = [];
const connection = createRegisteredApplication(Object.fromEntries(rows.map((row) => [row.identifier, {
  read() { const { identifier, ...state } = row; return state; },
  async click(context) {
    await Promise.resolve();
    context.commit(() => { row.value = "done"; row.enabled = false; row.clicks++; context.changed(row.identifier); });
  },
}])));
const application = {
  attach(bridge) {
    assert.equal(attached, null);
    attached = bridge;
    connection.attach(bridge);
  },
  detach(bridge) { assert.equal(attached, bridge); connection.detach(bridge); attached = null; },
  observe() { observations++; return connection.observe(); },
};
const transport = {
  async describe() { return catalog; },
  async execute(run) {
    const id = starts++ === 0 ? "review" : "archive";
    assert.equal(run.source, prepared.prepare("complete_" + id, {}).plan.source);
    assert.deepEqual(Object.keys(run.arguments), []);
    scopes.push(run.scope);
    const before = structuredClone(rows);
    let serial = 0;
    const call = (operation, arguments_ = {}) => new Promise((resolve) => {
      deliver = resolve;
      run.onRequest({ transport_version: 1, ...run.scope, request_id: ++serial, budget_ms: 1000,
        operation, arguments: arguments_ });
    });
    const target = await call("gui.find", { name: id });
    assert.equal(target.status, "ok");
    const observed = await call("gui.observe", { target: target.value });
    assert.equal(observed.status, "ok");
    assert.equal(observed.value.value, "pending");
    const action = await call("gui.click", { target: target.value, expected_revision: observed.value.revision });
    assert.equal(action.status, "ok", action.error);
    const after = await call("gui.observe", { target: target.value });
    assert.equal(after.status, "ok");
    assert.equal(after.value.value, "done");
    return JSON.stringify({ schema_version: 1, kind: "external_task_run", profile: "external.registered",
      result: { type: "string", value: "done" }, effects: 4, fuel_remaining: 9000,
      objects_before: before, objects_after: structuredClone(rows) });
  },
  reply(reply) { deliver(reply); },
  cancel() { assert.fail("successful run was cancelled"); },
  close(endpoint) { assert.deepEqual(endpoint, { window: "external", epoch: "fresh-instance" }); },
};
const host = createRegisteredAgentHost({ window: "external", epoch: "fresh-instance", application, transport,
  executionReports: "v1",
  observations: { before: "objects_before", after: "objects_after" }, onSource(source) { sources.push(source); } });
assert.equal(host.executionReports, "v1");
const session = new AgentSession({ host, provider: createSemanticProvider() });
for (const id of ["review", "archive"]) {
  const outcome = await session.submit("task complete_" + id);
  assert.equal(outcome.status, "verified", outcome.error);
  assert.equal(outcome.possiblePartialEffects, false);
  assert.equal(attached, null);
  assert.equal(host.verify(outcome.report, outcome.plan, new AbortController().signal), false, "evidence is single-use");
}
assert.equal(starts, 2);
assert.notEqual(scopes[0].session, scopes[1].session);
assert.equal(observations, 4, "independent before/current observations on each run");
assert.equal(sources.length, 2);
assert.deepEqual(rows.map(({ value, clicks, enabled }) => ({ value, clicks, enabled })),
  [{ value: "done", clicks: 1, enabled: false }, { value: "done", clicks: 1, enabled: false }]);
const invalid = await session.submit("task save_message:wrong application");
assert.equal(invalid.status, "failed");
assert.equal(invalid.possiblePartialEffects, false);
assert.equal(starts, 2);
host.close();
await assert.rejects(host.describe(new AbortController().signal));
`);
    run(process.execPath, ["registered-consumer.mjs"], directory);

    await writeFile(join(directory, "consumer.ts"), `
import { Bridge, AgentSession, type Scope, type Adapter, type ActionContext, type Reply,
  type AgentProvider, type AgentHost, type AgentOutcome, type WorkflowPlan,
  prepareSemanticCatalog, createSemanticProvider, type SemanticCatalog, type SemanticTaskInfo,
  type PreparedSemanticTask, type SemanticProviderOptions, type SemanticReportOptions, createModelProvider,
  type ModelProviderOptions, type ModelRequest, AgentWorkflowSession, createModelWorkflowProvider,
  type AgentWorkflowProvider, type AgentWorkflowPlan, type AgentWorkflowOutcome, type AgentWorkflowState, type AgentWorkflowSessionOptions, prepareSemanticWorkflow, type SemanticWorkflowStep,
  type WorkflowResultReference, type WorkflowResultFormat, type AgentWorkflowBinding, type ModelWorkflowProviderOptions,
  type WorkflowResultRequirement, type AgentWorkflowRequirement, type AgentWorkflowRequirementOutcome,
  createRegisteredAgentHost, createRegisteredApplication, createDynamicRegisteredApplication, type RegisteredApplication, type DynamicRegisteredApplication,
  type RegisteredControl, type RegisteredActionContext, type RegisteredApplicationConnection, type RegisteredAgentTransport,
  type RegisteredAgentRun, type RegisteredAgentEndpoint, type RegisteredAgentHostOptions, type RegisteredAgentHost } from "@leselang/bridge";
import { createNodeAgentTransport, createNodeOperationHost, type NodeAgentTransportOptions, type NodeOperationHostOptions } from "@leselang/bridge/node";
import type { OperationRegistry } from "@leselang/bridge/operations";
import { defineSemanticCatalog, defineRegisteredTask, defineRegisteredTaskV3, type RegisteredTaskDefinition, type RegisteredTaskDefinitionV3, type RegisteredTaskStep,
  type SemanticDescription, type SemanticDescriptionV1, type SemanticDescriptionV2, type SemanticDescriptionV3,
  type SemanticTaskDescriptionV1, type SemanticTaskDescriptionV2, type SemanticLiteral,
  type SemanticTerm, type SemanticReportPredicate, type SemanticObservedResult } from "@leselang/bridge";
import { createDomButtonControl, createDomTextControl } from "@leselang/bridge/dom";
const domButtonElement = document.createElement("button");
domButtonElement.type = "button";
const domButton = createDomButtonControl({
  element: domButtonElement,
  read: () => ({ role: "button", title: "Save", value: "", enabled: true, editable: false, clicks: 0 }),
  changed: (...names: string[]) => { void names; },
  async onClick(context) { await Promise.resolve(); context.commit(() => context.changed("save")); },
  beforeCommit() {}, onError(error, origin) { const source: "agent" | "manual" = origin; void error; void source; },
});
const domText = createDomTextControl({
  element: document.createElement("textarea"),
  read: () => ({ role: "text_field", title: "Draft", value: "", enabled: true, editable: true, clicks: 0 }),
  changed: (...names: string[]) => { void names; },
  validateText(text) { if (text.includes("\\0")) throw new Error("invalid text"); },
  onInput(text, context) {
    context.changed("draft", "save"); void text; void context.signal;
    // @ts-expect-error text handlers already run inside a commit and cannot nest one
    context.commit(() => undefined);
  },
});
const domApplication = createRegisteredApplication({ draft: domText.control, save: domButton.control });
domButton.dispose(); domText.dispose(); void domApplication;
// @ts-expect-error DOM builders are opt-in, not root exports
import { createDomButtonControl as RootDom } from "@leselang/bridge";
// @ts-expect-error a button binding requires a button element
const wrongDomElement: Parameters<typeof createDomButtonControl>[0]["element"] = document.createElement("textarea");
void RootDom; void wrongDomElement;
const exactLiteral: SemanticLiteral = { kind: "integer", value: 18_446_744_073_709_551_615n };
const exactTerm: SemanticTerm = { kind: "literal", value: exactLiteral };
const predicate: SemanticReportPredicate = { id: "same", left: { kind: "length", path: [{ kind: "field", name: "before" }] },
  relation: { kind: "equal" }, right: { kind: "length", path: [{ kind: "field", name: "after" }] } };
const fixedTask: SemanticTaskDescriptionV1 = { id: "read", summary: "Read declared value", inputs: [], execution: "no_text",
  steps: [{ kind: "call", binding: "value", operation: "sample.read", arguments: [] }], result: exactTerm,
  verification: { kind: "sample_run", max_fuel: 10_000, require_verified: false,
    result: exactLiteral, predicates: [predicate] } };
const observedResult: SemanticObservedResult = { kind: "observed", scalar_type: "integer", path: [{ kind: "field", name: "observed" }] };
const observedTask: SemanticTaskDescriptionV2 = { ...fixedTask, result: { kind: "binding", name: "value" },
  verification: { ...fixedTask.verification, result: observedResult } };
const registeredStep: RegisteredTaskStep = { kind: "set_control_text", control: "draft", binding: "after",
  before: "before", text: { kind: "input", name: "message" } };
const registeredDefinition: RegisteredTaskDefinition = { ...fixedTask, failure: { kind: "integer", value: 0 },
  steps: [registeredStep, { kind: "read_control", control: "draft", binding: "observed" }] };
const expand: (definition: RegisteredTaskDefinition) => SemanticTaskDescriptionV2 = defineRegisteredTask;
void expand; void registeredDefinition;
// @ts-expect-error an expanded task always requires an explicit guard failure result
const noFailure: RegisteredTaskDefinition = fixedTask;
// @ts-expect-error text operands are declared semantic terms, not interpolated source strings
const rawText: RegisteredTaskStep = { kind: "set_control_text", control: "draft", binding: "after", text: "message" };
// @ts-expect-error the declaration step list remains readonly
registeredDefinition.steps.push(registeredStep);
void noFailure; void rawText;
const v1: SemanticDescriptionV1 = { version: 1, operations: [], tasks: [fixedTask] };
const v2: SemanticDescriptionV2 = { version: 2, operations: [], tasks: [observedTask, fixedTask] };
const namedDefinition: RegisteredTaskDefinitionV3 = { ...registeredDefinition,
  inputs: [{ name: "title", scalar_type: "string" }, { name: "content", scalar_type: "string" }],
  execution: "named_strings" };
const namedTask = defineRegisteredTaskV3;
const v3: SemanticDescriptionV3 = { version: 3, operations: [], tasks: [{ ...fixedTask,
  inputs: namedDefinition.inputs, execution: "named_strings" }] };
const namedPlan: WorkflowPlan = { summary: "Two fields", source: "source", inputMode: "named_strings",
  arguments: { title: "literal title", content: "literal content" } };
void namedTask; void namedDefinition; void v3; void namedPlan;
// @ts-expect-error v2 must opt in to a separate task type for multi-input execution
const oldNamedTask: SemanticTaskDescriptionV2 = { ...fixedTask, execution: "named_strings" };
// @ts-expect-error an unknown input transport mode cannot be selected
const invalidMode: WorkflowPlan = { ...namedPlan, inputMode: "automatic" };
void oldNamedTask; void invalidMode;
const author: (base: unknown, semantics: SemanticDescription) => string = defineSemanticCatalog;
void author; void v1; void v2;
// @ts-expect-error v1 cannot declare an observed result
const invalidV1: SemanticDescriptionV1 = { version: 1, operations: [], tasks: [observedTask] };
// @ts-expect-error u64 authoring takes an exact bigint or safe Number, never a numeric string
const invalidInteger: SemanticLiteral = { kind: "integer", value: "18446744073709551615" };
// @ts-expect-error optional strings and lists are not observed result contracts
const invalidObserved: SemanticObservedResult = { kind: "observed", scalar_type: "string_list", path: [] };
// @ts-expect-error declarations are readonly
v1.tasks.push(fixedTask);
void invalidV1; void invalidInteger; void invalidObserved;
const nodeOptions: NodeAgentTransportOptions = {
  executable: "/trusted/agent_worker", args: [], timeoutMs: 15_000,
  executionReports: "v1",
  catalog(base: string): string { return base; }, onStderr(chunk: string): void { void chunk; },
};
const nodeTransport: RegisteredAgentTransport = createNodeAgentTransport(nodeOptions);
void nodeTransport;
declare const registry: OperationRegistry;
const operationOptions = { executable: "/trusted/worker", registry, grants: () => [] };
createNodeOperationHost({ ...operationOptions, verify({ report }) {
  const profile: "javascript.operations" = report.profile;
  return !!profile;
} });
const neutralOptions: NodeOperationHostOptions = { ...operationOptions, profile: "lese.operations", verify({ report }) {
  const profile: "lese.operations" = report.profile;
  return !!profile;
} };
createNodeOperationHost(neutralOptions);
// @ts-expect-error the worker supports only the two declared operation profiles
createNodeOperationHost({ ...operationOptions, profile: "other.operations", verify: () => true });
// @ts-expect-error the catalog callback must return raw JSON synchronously
const asyncNodeOptions: NodeAgentTransportOptions = { executable: "/trusted/agent_worker", catalog: async () => "{}" };
// @ts-expect-error execution reports require the explicit supported dialect
const invalidNodeReports: NodeAgentTransportOptions = { executable: "/trusted/agent_worker", catalog: (base) => base, executionReports: "v2" };
// @ts-expect-error Node APIs are available only through the explicit node subpath
import { createNodeAgentTransport as BrowserTransport } from "@leselang/bridge";
void asyncNodeOptions; void invalidNodeReports; void BrowserTransport;
const connectRegistered = (application: RegisteredApplicationConnection, transport: RegisteredAgentTransport): RegisteredAgentHost => {
  const options: RegisteredAgentHostOptions = { window: "worker", epoch: "fresh-instance", application, transport,
    executionReports: "v1",
    observations: { before: "objects_before", after: "objects_after" } };
  return createRegisteredAgentHost(options);
};
const routeRegistered = (run: RegisteredAgentRun, endpoint: RegisteredAgentEndpoint) => {
  run.onRequest({});
  const source: string = run.source;
  const scope: Readonly<Scope> = run.scope;
  const name: string = endpoint.window;
  void source; void scope; void name;
  // @ts-expect-error a transport cannot rewrite admitted source
  run.source = "different";
};
void connectRegistered; void routeRegistered;
const modelOptions: ModelProviderOptions = { async complete(request: ModelRequest, signal: AbortSignal): Promise<string> {
  void signal; return request.user;
} };
const modelProvider: AgentProvider = createModelProvider(modelOptions);
void modelProvider;
const workflowProvider: AgentWorkflowProvider = createModelWorkflowProvider(modelOptions);
const workflowPlan: AgentWorkflowPlan = { steps: [] };
const workflowOptions = (host: AgentHost): AgentWorkflowSessionOptions => ({ provider: workflowProvider, host, onEvent(state: AgentWorkflowState) { void state.completedSteps; } });
const runWorkflow = (host: AgentHost): Promise<AgentWorkflowOutcome> => new AgentWorkflowSession(workflowOptions(host)).submit("workflow");
void workflowPlan; void runWorkflow;
const bindingFormat: WorkflowResultFormat = "decimal";
const reference: WorkflowResultReference = { fromStep: 0, format: bindingFormat };
const semanticStep: SemanticWorkflowStep = { task: "save", arguments: { content: reference } };
const binding: AgentWorkflowBinding = { step: 1, argument: "content", ...reference };
const prepareWorkflow: (catalog: unknown, steps: readonly SemanticWorkflowStep[]) => AgentWorkflowPlan = prepareSemanticWorkflow;
const bindingOptions: ModelWorkflowProviderOptions = { ...modelOptions, resultBindings: true };
void semanticStep; void binding; void prepareWorkflow; void bindingOptions;
const requirement: WorkflowResultRequirement = { fromStep: 0, format: "decimal", equals: "3" };
const requiredStep: SemanticWorkflowStep = { ...semanticStep, requires: requirement };
const required: AgentWorkflowRequirement = { step: 1, ...requirement };
const decision: AgentWorkflowRequirementOutcome = { ...required, actual: "3", satisfied: true };
const requiredPlan: AgentWorkflowPlan = { steps: [], requirements: [required] };
const requirementOptions: ModelWorkflowProviderOptions = { ...modelOptions, resultRequirements: true };
const decisions = (outcome: AgentWorkflowOutcome): readonly AgentWorkflowRequirementOutcome[] => outcome.requirements ?? [];
void requiredStep; void decision; void requiredPlan; void requirementOptions; void decisions;
// @ts-expect-error expected text uses explicit conversion, not implicit numeric coercion
const invalidRequirement: WorkflowResultRequirement = { fromStep: 0, format: "decimal", equals: 3 };
void invalidRequirement;
// @ts-expect-error bindings require an explicit supported format
const invalidReference: WorkflowResultReference = { fromStep: 0, format: "auto" };
// @ts-expect-error original literal task arguments remain strings
const invalidArgument: WorkflowPlan = { summary: "Save", source: "source", arguments: { content: reference } };
void invalidReference; void invalidArgument;
const semanticOptions: SemanticProviderOptions = { aliases: { save: "publish" } };
const semanticProvider: AgentProvider = createSemanticProvider(semanticOptions);
const compileCatalog: (data: unknown) => SemanticCatalog = prepareSemanticCatalog;
const reportOptions: SemanticReportOptions = { executionReports: "v1" };
const compileConfiguredCatalog: (data: unknown, options?: SemanticReportOptions) => SemanticCatalog = prepareSemanticCatalog;
// @ts-expect-error report policy is a trusted readonly declaration
reportOptions.executionReports = undefined;
// @ts-expect-error unknown execution report versions require a new explicit contract
const wrongReportOptions: SemanticReportOptions = { executionReports: "v2" };
void reportOptions; void compileConfiguredCatalog; void wrongReportOptions;
const describeTask = (task: SemanticTaskInfo): string => task.id;
const candidate = (task: PreparedSemanticTask): WorkflowPlan => task.plan;
const detailed = (task: PreparedSemanticTask, report: unknown): import("@leselang/bridge").VerificationResult => task.verifyDetailed(report);
const failureCode: import("@leselang/bridge").VerificationFailureCode = "predicate_failed";
const diagnostic: import("@leselang/bridge").VerificationDiagnostic = { code: failureCode, predicate: "saved_title" };
const executionDiagnostic: import("@leselang/bridge").VerificationDiagnostic = { code: "execution_object_unavailable" };
// @ts-expect-error execution diagnostics retain the bounded code-only shape
const rawFailureText: import("@leselang/bridge").VerificationDiagnostic = { code: "execution_failed", error: "exception text" };
void executionDiagnostic; void rawFailureText;
// @ts-expect-error diagnostics use a closed set of machine-readable codes
const invalidDiagnostic: import("@leselang/bridge").VerificationDiagnostic = { code: "arbitrary message" };
// @ts-expect-error a failed result requires its diagnostic
const missingDiagnostic: import("@leselang/bridge").VerificationResult = { verified: false };
void detailed; void diagnostic; void invalidDiagnostic; void missingDiagnostic;
void semanticProvider; void compileCatalog; void describeTask; void candidate;
const scope: Scope = { window: "worker", epoch: "instance-1", session: "task-1" };
const bridge = new Bridge(scope);
const closed: boolean = bridge.closed;
const control: RegisteredControl = {
  read: () => ({ role: "button", title: "Update", value: "", enabled: true, editable: false, clicks: 0 }),
  async click(context: RegisteredActionContext) {
    await Promise.resolve();
    context.commit(() => { context.changed("control"); });
  },
};
const application: RegisteredApplication = createRegisteredApplication({ control });
const connection: RegisteredApplicationConnection = application;
const dynamicApplication: DynamicRegisteredApplication = createDynamicRegisteredApplication({ counter: null });
dynamicApplication.mount("counter", control);
dynamicApplication.replace("counter", control);
dynamicApplication.unmount("counter");
const bindingRevision: number = dynamicApplication.bindingRevision();
const dynamicConnection: RegisteredApplicationConnection = dynamicApplication;
// @ts-expect-error the static factory does not expose structural mutation methods
application.mount("counter", control);
// @ts-expect-error binding identity is a numeric stamp, not a serialized token
const wrongRevision: RegisteredApplicationConnection = { ...connection, bindingRevision: () => "0" };
void bindingRevision; void dynamicConnection; void wrongRevision;
application.changed("control");
void closed; void connection;
// @ts-expect-error action mutation remains an explicit synchronous callback
const invalidControl: RegisteredControl = { ...control, click(context: RegisteredActionContext) { context.commit(42); } };
// @ts-expect-error lifecycle state cannot be assigned by a consumer
bridge.closed = false;
void invalidControl;
const adapter: Adapter = {
  read: () => ({ role: "button", title: "Increment", value: "0", enabled: true, editable: false, clicks: 0 }),
  async click(context: ActionContext) { await Promise.resolve(); context.commit(() => undefined); },
};
bridge.register("counter", adapter);
const reply: Promise<Reply> = bridge.handle({});
void reply;
const provider: AgentProvider = { async plan({ message }) {
  return { summary: "Save", source: "fn main(text: string) = true", arguments: { text: message } };
} };
const host: AgentHost = { async describe() { return {}; }, async execute(plan) { return { saved: plan.arguments.text }; }, verify() { return true; } };
const configuredHost: AgentHost = { ...host, executionReports: "v1" };
// @ts-expect-error applications cannot select an unknown execution report contract
const wrongConfiguredHost: AgentHost = { ...host, executionReports: "v2" };
// @ts-expect-error execution report declarations are readonly
configuredHost.executionReports = undefined;
void configuredHost; void wrongConfiguredHost;
const session = new AgentSession({ provider, host });
const outcome: Promise<AgentOutcome> = session.submit("task");
void outcome;
// @ts-expect-error task arguments remain strings
const invalidPlan: WorkflowPlan = { summary: "Save", source: "source", arguments: { text: 42 } };
void invalidPlan;
// @ts-expect-error endpoint labels are strings
const invalidScope: Scope = { window: 42, epoch: "page", session: "task" };
// @ts-expect-error an adapter must return typed scalar state
const invalidAdapter: Adapter = { read: () => ({ role: "button", title: "", value: "", enabled: "yes", editable: false, clicks: 0 }) };
// @ts-expect-error source implementation is not a public package export
import { Bridge as PrivateBridge } from "@leselang/bridge/src/index.js";
void invalidScope; void invalidAdapter; void PrivateBridge;
`);
    run("tsc", ["--noEmit", "--strict", "--target", "ES2022", "--module", "NodeNext", "--moduleResolution", "NodeNext", "consumer.ts"], directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
