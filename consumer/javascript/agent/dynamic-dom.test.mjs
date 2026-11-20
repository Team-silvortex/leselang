import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  prepareSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createDynamicDomArticleApplication } from "./dom-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

const tick = () => new Promise((resolve) => setImmediate(resolve));
const command = (args) => `task publish_article:${JSON.stringify(args)}`;
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
function fixture(options = {}, hooks = {}) {
  const application = createDynamicDomArticleApplication(options), requests = [], reports = [];
  let trace = "", catalog;
  const carrier = createNodeAgentTransport({ executable: defaultWorker, catalog: createArticleCatalog,
    onStderr(chunk) { trace += chunk; } });
  const host = createRegisteredAgentHost({ window: "dynamic-dom-consumer", epoch: randomUUID(), application,
    transport: { async describe(signal) { catalog = await carrier.describe(signal); return catalog; },
      reply: (reply) => carrier.reply(reply), cancel: (scope) => carrier.cancel(scope), close: (endpoint) => carrier.close(endpoint),
      async execute(run, signal) {
        const report = await carrier.execute({ ...run, onRequest(request) {
          requests.push(request); hooks.beforeRequest?.(application, request); run.onRequest(request);
        } }, signal);
        reports.push(report); hooks.afterExecute?.(application, report); return report;
      } }, observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentSession({ host, provider: createSemanticProvider() });
  return { application, host, session, requests, reports, trace: () => trace, catalog: () => catalog,
    close() { session.cancel(); host.close(); application.dispose(); } };
}

test("real worker discovers remounted same-name DOM controls with unchanged canonical programs", async () => {
  const app = fixture({ title: "Remounted title ✓", content: "Preserved draft content" });
  const original = app.application.elements.publish;
  app.application.unmountPublish();
  assert.equal(app.application.observe().some((row) => row.identifier === "publish"), false);
  app.application.mountPublish();
  assert.notEqual(app.application.elements.publish, original);
  assert.equal(original.isConnected, false);
  assert.equal(app.application.bindingRevision(), 2);
  const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} }, { task: "read_content", arguments: {} },
      { task: "publish_article", arguments: { title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" } } },
      { task: "check_article", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await workflow.submit("Publish the two observed fields after returning to the editor");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(outcome.steps.map((step) => JSON.parse(step.report).effects), [2, 2, 12, 1]);
    assert.equal(app.application.bindingRevision(), 2, "normal edits and attachment do not change structural identity");
    assert.equal(app.application.snapshot().publishes, 1);
    assert.equal(app.application.snapshot().otherClicks, 0);
    assert.equal((app.trace().match(/poll -> authorize -> dispatch:/gu) ?? []).length, 17);
    assert.equal((app.trace().match(/^accept$/gmu) ?? []).length, 17);
    const canonical = prepareSemanticCatalog(app.catalog()).prepare("publish_article", outcome.steps[2].plan.arguments).plan.source;
    assert.equal(outcome.steps[2].plan.source, canonical);
    const before = app.application.observe();
    app.application.remountPublish();
    assert.deepEqual(app.application.observe(), before, "an instance replacement can preserve every observed value");
    const next = await app.session.submit(command({ title: "Next explicit task", content: "Uses the replacement button" }));
    assert.equal(next.status, "verified", next.error);
    assert.equal(next.plan.source, canonical);
    assert.equal(app.application.snapshot().publishes, 2);
  } finally { workflow.cancel(); app.close(); }
});

for (const change of ["replace", "unmount_mount"]) {
  test(`a real completed report cannot verify after same-value ${change}`, async () => {
    let changed = false;
    const app = fixture({}, { afterExecute(application) {
      if (changed) return;
      changed = true;
      const observed = application.observe();
      if (change === "replace") application.remountPublish();
      else { application.unmountPublish(); application.mountPublish(); }
      assert.deepEqual(application.observe(), observed);
    } });
    const args = { title: "Already committed", content: "Retain this publication" };
    const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
      return prepareSemanticWorkflow(catalog, [
        { task: "publish_article", arguments: args }, { task: "read_content", arguments: {} },
      ]);
    } } });
    try {
      const outcome = await workflow.submit("Publish then inspect");
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.error, "application_verification_failed");
      assert.equal(outcome.steps.length, 1);
      assert.equal(outcome.possiblePartialEffects, true);
      assert.deepEqual(outcome.verificationFailure, { code: "application_bindings_changed" });
      assert.deepEqual(outcome.steps[0].verificationFailure, outcome.verificationFailure);
      assert.equal(app.reports.length, 1);
      const step = outcome.steps[0];
      assert.equal(JSON.parse(step.report).effects, 12);
      assert.equal(prepareSemanticCatalog(app.catalog()).prepare("publish_article", args).verify(step.report), true);
      assert.deepEqual(await app.host.verifyDetailed(step.report, step.plan, new AbortController().signal),
        { verified: false, diagnostic: { code: "completion_unavailable" } });
      assert.equal(app.application.snapshot().publishes, 1);
      const read = await app.session.submit("task read_content");
      assert.equal(read.status, "verified", read.error);
      assert.equal(JSON.parse(read.report).result.value, args.content);
      assert.equal(app.application.snapshot().publishes, 1, "verification failure never repeats a committed action");
    } finally { workflow.cancel(); app.close(); }
  });
}

test("replacing an awaited DOM fixture handler fences its old commit and the old task", async () => {
  const entered = deferred(), release = deferred();
  const app = fixture({ async beforePublish() { entered.resolve(); await release.promise; } });
  const args = { title: "Retained title", content: "Retained content" };
  try {
    const pending = app.session.submit(command(args));
    await Promise.race([entered.promise, pending.then((outcome) => assert.fail(`publication not entered: ${outcome.error}`))]);
    app.application.remountPublish();
    release.resolve();
    const outcome = await pending;
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.error, "application_bindings_changed");
    assert.equal(outcome.report, undefined);
    await tick();
    assert.equal(app.application.snapshot().publishes, 0);
    assert.deepEqual({ title: app.application.snapshot().title, content: app.application.snapshot().content }, args);
    const next = await app.session.submit(command(args));
    assert.equal(next.status, "verified", next.error);
    assert.equal(app.application.snapshot().publishes, 1);
  } finally { release.resolve(); app.close(); }
});

test("a binding change before the first worker request stops dispatch without entering a DOM handler", async () => {
  let changed = false;
  const app = fixture({}, { beforeRequest(application) {
    if (!changed) { changed = true; application.remountPublish(); }
  } });
  try {
    const outcome = await app.session.submit(command({ title: "Not entered", content: "Not entered" }));
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_bindings_changed");
    assert.equal(app.requests.length, 1);
    assert.deepEqual(app.application.eventCounts(), { title: 0, content: 0, publish: 0, other: 0 });
    assert.equal(outcome.report, undefined);
    const next = await app.session.submit("task read_title");
    assert.equal(next.status, "verified", next.error);
  } finally { app.close(); }
});

test("a late retired handler cannot mutate or invalidate a fresh real-worker task", async () => {
  const firstEntered = deferred(), firstRelease = deferred();
  const nextEntered = deferred(), nextRelease = deferred();
  let calls = 0;
  const app = fixture({ async beforePublish() {
    if (++calls === 1) { firstEntered.resolve(); await firstRelease.promise; }
    else { nextEntered.resolve(); await nextRelease.promise; }
  } });
  const first = { title: "Retired draft", content: "Never published" };
  const next = { title: "Fresh draft", content: "Published once by its own handler" };
  try {
    const retired = app.session.submit(command(first));
    await Promise.race([firstEntered.promise, retired.then((outcome) => assert.fail(`first handler not entered: ${outcome.error}`))]);
    app.application.remountPublish();
    app.session.cancel();
    assert.equal((await retired).status, "cancelled");
    const pending = app.session.submit(command(next));
    await Promise.race([nextEntered.promise, pending.then((outcome) => assert.fail(`new handler not entered: ${outcome.error}`))]);
    firstRelease.resolve();
    await tick(); await tick();
    assert.deepEqual(app.application.snapshot(), { ...next, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
    nextRelease.resolve();
    const outcome = await pending;
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(JSON.parse(outcome.report).effects, 12);
    assert.deepEqual(app.application.snapshot(), { ...next, savedTitle: next.title, savedContent: next.content,
      publishes: 1, otherClicks: 0 });
    assert.equal(calls, 2, "no retry or extra callback entered");
  } finally { firstRelease.resolve(); nextRelease.resolve(); app.close(); }
});
