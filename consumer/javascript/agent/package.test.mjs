// Acceptance from an installed tarball in a new directory, with the real VM.
// Only application modules are copied; the carrier comes from the installed package.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const sdk = fileURLToPath(new URL("../../../packages/javascript/", import.meta.url));
const worker = fileURLToPath(new URL("../../../target/lean/leselang-agent-worker", import.meta.url));

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", timeout: 30_000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

test("an external installed Node SDK consumer runs real workflows, partial failure and cancellation", { timeout: 60_000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), "leselang-node-package-"));
  try {
    const cache = join(directory, "empty-npm-cache");
    const [packed] = JSON.parse(run("npm", ["pack", "--ignore-scripts", "--offline", "--json",
      "--pack-destination", directory, "--cache", cache], sdk));
    await writeFile(join(directory, "package.json"), JSON.stringify({
      name: "external-node-agent-consumer", private: true, type: "module",
    }));
    run("npm", ["install", join(directory, packed.filename), "--offline", "--ignore-scripts",
      "--no-audit", "--no-fund", "--cache", cache], directory);
    const installed = JSON.parse(await readFile(join(directory, "node_modules/@leselang/bridge/package.json"), "utf8"));
    assert.deepEqual(installed.dependencies ?? {}, {}, "the public Node carrier has no runtime npm dependencies");
    assert.deepEqual((await readdir(join(directory, "node_modules"))).sort(), [".package-lock.json", "@leselang"]);

    await mkdir(join(directory, "agent"));
    for (const file of ["application.mjs", "catalog.mjs", "assistant.mjs", "multi-input-application.mjs", "multi-input-catalog.mjs", "dom-application.mjs"]) {
      await copyFile(new URL(file, import.meta.url), join(directory, "agent", file));
    }
    await copyFile(new URL("../semantic-contract.mjs", import.meta.url), join(directory, "semantic-contract.mjs"));
    await writeFile(join(directory, "consumer.mjs"), String.raw`
import assert from "node:assert/strict";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider, defineSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createPublicationApplication } from "./agent/application.mjs";
import { createPublicationAgent } from "./agent/assistant.mjs";
import { createArticleApplication } from "./agent/multi-input-application.mjs";
import { createArticleCatalog } from "./agent/multi-input-catalog.mjs";
import { createDomArticleApplication, createDynamicDomArticleApplication } from "./agent/dom-application.mjs";

assert.equal(typeof createNodeAgentTransport, "function");
assert.equal(Object.hasOwn(await import("@leselang/bridge"), "createNodeAgentTransport"), false);
await assert.rejects(import("@leselang/bridge/dist/node/index.js"), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
const executable = process.argv[2];
const report = (step) => JSON.parse(step.report);
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
function workflow(host, text) {
  return new AgentWorkflowSession({ host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_draft", arguments: {} },
      { task: "publish_article", arguments: { text: { fromStep: 0, format: "string" } },
        requires: { fromStep: 0, format: "string", equals: text } },
      { task: "read_publication", arguments: {} },
      { task: "check_publication", arguments: {} },
    ]);
  } } });
}
const acceptance = {};
{
  // Actual native classification through the installed optional Node entry.
  // The default carrier keeps the previous exception contract for the same fault.
  const outcomes = [];
  for (const executionReports of [undefined, "v1"]) {
    const application = createDynamicDomArticleApplication();
    application.unmountPublish();
    const transport = createNodeAgentTransport({ executable, catalog: createArticleCatalog, executionReports });
    const host = createRegisteredAgentHost({ window: "installed-native-failure", epoch: "fresh-instance", application,
      transport, executionReports: "v1", observations: { before: "objects_before", after: "objects_after" } });
    const session = new AgentSession({ host, provider: createSemanticProvider() });
    const args = { title: "Installed retained title", content: "Installed retained content" };
    try {
      const outcome = await session.submit("task publish_article:" + JSON.stringify(args));
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.possiblePartialEffects, true);
      assert.deepEqual(application.snapshot(), { ...args, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
      if (executionReports) {
        assert.equal(outcome.error, "application_verification_failed");
        assert.deepEqual(outcome.verificationFailure, { code: "execution_object_unavailable" });
        assert.deepEqual(report(outcome), { schema_version: 1, kind: "javascript_task_run", profile: "javascript.registered",
          status: "failed", error: "unknown registered object", possible_partial_effects: true,
          execution_failure: { code: "execution_object_unavailable" } });
      } else {
        assert.equal(outcome.error, "node_worker_failed: unknown registered object");
        assert.equal(outcome.report, undefined);
        assert.equal(outcome.verificationFailure, undefined);
      }
      application.mountPublish();
      const next = await session.submit("task publish_article:" + JSON.stringify(args));
      assert.equal(next.status, "verified", next.error);
      assert.equal(report(next).effects, 12);
      assert.equal(application.snapshot().publishes, 1);
      outcomes.push({ mode: executionReports ?? "legacy", code: outcome.verificationFailure?.code ?? null,
        subsequentRun: next.status, publications: 1 });
    } finally { session.cancel(); host.close(); application.dispose(); }
  }
  acceptance.nativeFailure = outcomes;
}
{
  // Inject a terminal failure around a real worker report. This checks public
  // report admission from the installed tarball, not a new Node wire format.
  const application = createArticleApplication({ title: "Failed source", content: "Must remain a draft" });
  const carrier = createNodeAgentTransport({ executable, catalog: createArticleCatalog });
  let inject = true, executions = 0;
  const host = createRegisteredAgentHost({ window: "installed-failure", epoch: "fresh-instance", application,
    executionReports: "v1", observations: { before: "objects_before", after: "objects_after" },
    transport: { describe: (signal) => carrier.describe(signal), reply: (reply) => carrier.reply(reply),
      cancel: (scope) => carrier.cancel(scope), close: (endpoint) => carrier.close(endpoint),
      async execute(run, signal) {
        executions++;
        const raw = await carrier.execute(run, signal);
        return inject ? JSON.stringify({ ...JSON.parse(raw), status: "failed", error: "injected terminal failure",
          possible_partial_effects: false, execution_failure: { code: "execution_failed" } }) : raw;
      } } });
  const session = new AgentWorkflowSession({ host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} },
      { task: "publish_article", arguments: { title: { fromStep: 0, format: "string" }, content: "Never publish" } },
    ]);
  } } });
  try {
    const result = await session.submit("Refuse terminal failure before binding the source value");
    assert.equal(result.status, "failed");
    assert.deepEqual(result.verificationFailure, { code: "execution_failed" });
    assert.equal(result.steps.length, 1);
    assert.equal(executions, 1);
    assert.equal(report(result.steps[0]).effects, 2);
    assert.equal(result.possiblePartialEffects, true);
    assert.equal(application.snapshot().publishes, 0);
    inject = false;
    const single = new AgentSession({ host, provider: createSemanticProvider() });
    const read = await single.submit("task read_title");
    assert.equal(read.status, "verified", read.error);
    acceptance.executionFailure = { code: result.verificationFailure.code, attempted: result.steps.length,
      publications: 0, subsequentRead: read.status };
  } finally { session.cancel(); host.close(); }
}
{
  const application = createDynamicDomArticleApplication();
  application.unmountPublish(); application.mountPublish();
  assert.equal(application.bindingRevision(), 2);
  const transport = createNodeAgentTransport({ executable, catalog: createArticleCatalog });
  const host = createRegisteredAgentHost({ window: "installed-dynamic", epoch: "fresh-instance", application, transport,
    observations: { before: "objects_before", after: "objects_after" } });
  let replaceBeforeVerify = false;
  const session = new AgentSession({ host, provider: createSemanticProvider(), onEvent(state) {
    if (replaceBeforeVerify && state.phase === "verifying") application.remountPublish();
  } });
  try {
    const command = 'task publish_article:{"title":"Remounted installed title","content":"Preserved installed content"}';
    const first = await session.submit(command);
    assert.equal(first.status, "verified", first.error);
    assert.equal(report(first).effects, 12);
    replaceBeforeVerify = true;
    const failed = await session.submit(command);
    assert.equal(failed.status, "failed");
    assert.equal(failed.error, "application_verification_failed");
    assert.deepEqual(failed.verificationFailure, { code: "application_bindings_changed" });
    assert.equal(report(failed).effects, 12);
    assert.equal(failed.possiblePartialEffects, true);
    assert.equal(application.snapshot().publishes, 2);
    replaceBeforeVerify = false;
    const read = await session.submit("task read_title");
    assert.equal(read.status, "verified", read.error);
    assert.equal(application.snapshot().publishes, 2);
    acceptance.dynamic = { status: first.status, failure: failed.verificationFailure.code, publications: 2, subsequentRead: read.status };
  } finally { session.cancel(); host.close(); application.dispose(); }
}
{
  // EventTarget fixture + installed public DOM helpers + actual Rust worker.
  // This does not emulate browser layout or claim a native GUI interaction.
  const application = createDomArticleApplication();
  const initial = { title: "Installed DOM title ✓", content: "Installed event content" };
  application.manualInput("title", initial.title);
  application.manualInput("content", initial.content);
  const transport = createNodeAgentTransport({ executable, catalog: createArticleCatalog });
  const host = createRegisteredAgentHost({ window: "installed-dom", epoch: "fresh-instance", application, transport,
    observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentWorkflowSession({ host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} }, { task: "read_content", arguments: {} },
      { task: "publish_article", arguments: { title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" } } },
      { task: "check_article", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await session.submit("Use the installed DOM adapter to publish both verified fields");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(outcome.steps.map((step) => report(step).effects), [2, 2, 12, 1]);
    assert.deepEqual(application.snapshot(), { ...initial, savedTitle: initial.title, savedContent: initial.content, publishes: 1, otherClicks: 0 });
    assert.deepEqual(application.eventCounts(), { title: 2, content: 2, publish: 1, other: 0 });
    assert.equal(application.elements.savedContent.textContent, initial.content);
    acceptance.dom = { status: outcome.status, effects: 17, publishes: 1, handlers: application.eventCounts() };
  } finally { session.cancel(); host.close(); application.dispose(); }
}
{
  const application = createArticleApplication();
  const transport = createNodeAgentTransport({ executable, catalog(base) {
    const { semantics, ...catalog } = JSON.parse(createArticleCatalog(base));
    semantics.tasks.find((task) => task.id === "publish_article").verification.predicates.push({
      id: "requires_review", left: { kind: "path", path: [
        { kind: "field", name: "objects_after" }, { kind: "match", field: "identifier", value: "publish" },
        { kind: "field", name: "clicks" },
      ] }, relation: { kind: "equal" }, right: { kind: "literal", value: { kind: "integer", value: 2 } },
    });
    return defineSemanticCatalog(catalog, semantics);
  } });
  const host = createRegisteredAgentHost({ window: "installed-diagnostic", epoch: "fresh-instance", application, transport,
    observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentWorkflowSession({ host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "publish_article", arguments: { title: "Installed title", content: "Installed content" } },
      { task: "read_content", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await session.submit("Stop after the failed business condition");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.equal(outcome.steps.length, 1);
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(outcome.verificationFailure, { code: "predicate_failed", predicate: "requires_review" });
    assert.deepEqual(outcome.steps[0].verificationFailure, outcome.verificationFailure);
    assert.ok(Object.isFrozen(outcome.verificationFailure));
    assert.equal(report(outcome.steps[0]).effects, 12);
    assert.equal(application.snapshot().publishes, 1);
    acceptance.diagnostic = { status: outcome.status, failure: outcome.verificationFailure, effects: 12, publishes: 1 };
  } finally { session.cancel(); host.close(); }
}
{
  const initial = { title: 'Installed "title" ✓', content: 'Separate content \\ path\nnext line' };
  const application = createArticleApplication(initial);
  const transport = createNodeAgentTransport({ executable, catalog: createArticleCatalog });
  const host = createRegisteredAgentHost({ window: "installed-named", epoch: "fresh-instance", application, transport,
    observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentWorkflowSession({ host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} },
      { task: "read_content", arguments: {} },
      { task: "publish_article", arguments: { title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" } } },
      { task: "check_article", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await session.submit("Publish the two verified fields");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(outcome.steps.map((step) => report(step).effects), [2, 2, 12, 1]);
    assert.ok(outcome.steps.every((step) => step.plan.inputMode === "named_strings"));
    assert.deepEqual({ ...outcome.steps[2].plan.arguments }, initial);
    assert.deepEqual(application.snapshot(), { ...initial, savedTitle: initial.title, savedContent: initial.content, publishes: 1, otherClicks: 0 });
    acceptance.named = { status: outcome.status, effects: 17, publishes: 1 };
  } finally { session.cancel(); host.close(); }
}
{
  const text = 'Packed external "应用" \\ path\nsecond line ✓';
  const application = createPublicationApplication({ initialDraft: text });
  let trace = "";
  const app = createPublicationAgent({ executable, application, onStderr: (chunk) => { trace += chunk; } });
  const session = workflow(app.host, text);
  try {
    const outcome = await session.submit("run the declared offline workflow");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(outcome.possiblePartialEffects, false);
    assert.deepEqual(outcome.steps.map((step) => report(step).effects), [2, 9, 2, 1]);
    assert.deepEqual(outcome.steps.map((step) => report(step).result), [
      { type: "string", value: text }, { type: "boolean", value: true },
      { type: "string", value: text }, { type: "none", value: null },
    ]);
    assert.equal(outcome.requirements[0].satisfied, true);
    assert.equal(outcome.requirements[0].actual, text);
    assert.equal(outcome.steps[1].plan.arguments.text, text);
    assert.equal(outcome.steps[1].plan.source.includes(text), false);
    assert.equal((trace.match(/poll -> authorize -> dispatch:/gu) ?? []).length, 14);
    assert.equal((trace.match(/^accept$/gmu) ?? []).length, 14);
    for (const step of outcome.steps) {
      const value = report(step);
      assert.equal(value.profile, "javascript.registered");
      assert.equal(value.kind, "javascript_task_run");
      assert.ok(value.fuel_remaining > 0 && value.fuel_remaining < 10_000);
      assert.equal(Object.hasOwn(value, "verified"), false);
    }
    assert.deepEqual(application.snapshot(), { draft: text, published: text, status: "published", publishes: 1, resets: 0, unrelated: 0 });
    acceptance.workflow = { status: outcome.status, effects: 14, publishes: application.snapshot().publishes };
  } finally { session.cancel(); app.close(); }
}
{
  const text = "already published through the installed package ✓";
  const application = createPublicationApplication({ initialDraft: text, faultAfterCommit: true });
  const app = createPublicationAgent({ executable, application });
  const session = workflow(app.host, text);
  try {
    const outcome = await session.submit("publish then verify");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(outcome.steps.map((step) => step.status), ["verified", "failed"]);
    assert.equal(outcome.steps[1].report, undefined);
    assert.deepEqual(application.snapshot(), { draft: text, published: text, status: "published", publishes: 1, resets: 0, unrelated: 0 });
    const observed = await app.session.submit("task read_publication");
    assert.equal(observed.status, "verified", observed.error);
    assert.equal(report(observed).result.value, text);
    assert.equal(application.snapshot().publishes, 1, "failure never retries a committed action");
    acceptance.partial = { status: outcome.status, possiblePartialEffects: true, publishes: 1 };
  } finally { session.cancel(); app.close(); }
}
{
  const entered = deferred(), release = deferred();
  const application = createPublicationApplication({ initialDraft: "before", async beforePublish() {
    entered.resolve(); await release.promise;
  } });
  const app = createPublicationAgent({ executable, application });
  try {
    const pending = app.session.submit("task publish_article:earlier committed draft");
    await Promise.race([entered.promise, pending.then((outcome) => assert.fail("action not entered: " + outcome.error))]);
    assert.equal(app.session.cancel(), true);
    const outcome = await pending;
    assert.equal(outcome.status, "cancelled");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(application.snapshot().draft, "earlier committed draft");
    assert.equal(application.snapshot().publishes, 0);
    release.resolve(); await tick(); await tick();
    assert.equal(application.snapshot().publishes, 0, "a late continuation cannot commit");
    const read = await app.session.submit("task read_draft");
    assert.equal(read.status, "verified", read.error);
    assert.equal(report(read).result.value, "earlier committed draft");
    acceptance.cancelled = { status: outcome.status, possiblePartialEffects: true, publishes: 0, subsequentRead: read.status };
  } finally { release.resolve(); app.close(); }
}
process.stdout.write(JSON.stringify(acceptance) + "\n");
`);
    const acceptance = JSON.parse(run(process.execPath, ["consumer.mjs", worker], directory));
    assert.deepEqual(acceptance, {
      nativeFailure: [
        { mode: "legacy", code: null, subsequentRun: "verified", publications: 1 },
        { mode: "v1", code: "execution_object_unavailable", subsequentRun: "verified", publications: 1 },
      ],
      executionFailure: { code: "execution_failed", attempted: 1, publications: 0, subsequentRead: "verified" },
      dynamic: { status: "verified", failure: "application_bindings_changed", publications: 2, subsequentRead: "verified" },
      dom: { status: "verified", effects: 17, publishes: 1, handlers: { title: 2, content: 2, publish: 1, other: 0 } },
      diagnostic: { status: "failed", failure: { code: "predicate_failed", predicate: "requires_review" }, effects: 12, publishes: 1 },
      named: { status: "verified", effects: 17, publishes: 1 },
      workflow: { status: "verified", effects: 14, publishes: 1 },
      partial: { status: "failed", possiblePartialEffects: true, publishes: 1 },
      cancelled: { status: "cancelled", possiblePartialEffects: true, publishes: 0, subsequentRead: "verified" },
    });

    await writeFile(join(directory, "consumer.ts"), `
import { createNodeAgentTransport, type NodeAgentTransportOptions } from "@leselang/bridge/node";
import { createRegisteredAgentHost, createRegisteredApplication, type RegisteredAgentTransport, type VerificationResult } from "@leselang/bridge";
const options: NodeAgentTransportOptions = {
  executable: "/trusted/agent_worker", args: [], timeoutMs: 15_000, executionReports: "v1",
  catalog(base: string): string { return base; },
  onStderr(text: string): void { void text; },
};
const transport: RegisteredAgentTransport = createNodeAgentTransport(options);
const host = createRegisteredAgentHost({
  window: "typed-node", epoch: "fresh-instance", transport,
  application: createRegisteredApplication({}),
  executionReports: "v1",
  observations: { before: "objects_before", after: "objects_after" },
});
void host;
const check = (report: Parameters<typeof host.verify>[0], plan: Parameters<typeof host.verify>[1], signal: AbortSignal): VerificationResult | Promise<VerificationResult> => host.verifyDetailed(report, plan, signal);
void check;
// @ts-expect-error worker paths are strings
const badExecutable: NodeAgentTransportOptions = { executable: 123, catalog: (base) => base };
// @ts-expect-error execution reports require the supported explicit dialect
const badReports: NodeAgentTransportOptions = { executable: "/trusted/agent_worker", catalog: (base) => base, executionReports: "v2" };
// @ts-expect-error only the optional Node subpath exports a process carrier
import { createNodeAgentTransport as rootCarrier } from "@leselang/bridge";
void badExecutable; void badReports; void rootCarrier;
`);
    await writeFile(join(directory, "tsconfig.json"), JSON.stringify({
      compilerOptions: { noEmit: true, strict: true, target: "ES2022", module: "NodeNext", moduleResolution: "NodeNext",
        lib: ["ES2022", "DOM"], types: [], allowJs: true, checkJs: true, noUncheckedIndexedAccess: true },
      files: ["consumer.ts", "semantic-contract.mjs", "agent/catalog.mjs", "agent/application.mjs",
        "agent/multi-input-catalog.mjs", "agent/multi-input-application.mjs"],
    }));
    run(process.execPath, [join(sdk, "node_modules/typescript/bin/tsc"), "--project", "tsconfig.json"], directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
