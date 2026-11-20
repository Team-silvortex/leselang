import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  prepareSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createDynamicDomArticleApplication } from "./dom-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { createPublicationApplication } from "./application.mjs";
import { createPublicationAgent, defaultWorker } from "./assistant.mjs";

const signal = () => new AbortController().signal;
const command = (args) => `task publish_article:${JSON.stringify(args)}`;
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}

// The application is an EventTarget fixture. Both failures and successful reports
// below come directly from the real Rust worker, without report decoration.
function fixture(options = {}, rewriteReply = (reply) => reply) {
  const application = createDynamicDomArticleApplication(options), reports = [], bridges = [], runs = [];
  let catalog;
  const carrier = createNodeAgentTransport({ executable: defaultWorker, catalog: createArticleCatalog,
    executionReports: "v1" });
  const host = createRegisteredAgentHost({ window: "node-reports", epoch: randomUUID(),
    application: { ...application, attach(bridge) { bridges.push(bridge); application.attach(bridge); } },
    executionReports: "v1", observations: { before: "objects_before", after: "objects_after" },
    transport: {
      async describe(abort) { catalog = await carrier.describe(abort); return catalog; },
      reply: (reply) => carrier.reply(rewriteReply(reply)),
      cancel: (scope) => carrier.cancel(scope), close: (endpoint) => carrier.close(endpoint),
      async execute(run, abort) {
        runs.push(run);
        const raw = await carrier.execute(run, abort);
        reports.push(raw); return raw;
      },
    } });
  const session = new AgentSession({ host, provider: createSemanticProvider() });
  return { application, host, session, carrier, reports, bridges, runs, catalog: () => catalog,
    close() { session.cancel(); host.close(); application.dispose(); } };
}

test("worker3 reports a missing control after retained edits, stops the workflow and allows explicit recovery", async () => {
  const app = fixture({ title: "Before", content: "Before content" });
  const args = { title: "Retained title ✓", content: "Two edits survive\nmissing publication control" };
  app.application.unmountPublish();
  const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} },
      { task: "publish_article", arguments: args },
      { task: "read_content", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await workflow.submit("Read, publish, then inspect");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.deepEqual(outcome.steps.map((step) => step.status), ["verified", "failed"]);
    assert.deepEqual(outcome.verificationFailure, { code: "execution_object_unavailable" });
    assert.deepEqual(outcome.steps[1].verificationFailure, outcome.verificationFailure);
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(app.runs.length, 2, "the final read does not start after failure");
    assert.equal(JSON.parse(outcome.steps[0].report).effects, 2);
    assert.equal(outcome.steps[1].report, app.reports[1], "the worker's original report bytes are retained");
    assert.deepEqual(JSON.parse(app.reports[1]), {
      schema_version: 1, kind: "javascript_task_run", profile: "javascript.registered", status: "failed",
      error: "unknown registered object", possible_partial_effects: true,
      execution_failure: { code: "execution_object_unavailable" },
    });
    assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
    assert.deepEqual(app.application.eventCounts(), { title: 1, content: 1, publish: 0, other: 0 });
    assert.deepEqual(await app.host.verifyDetailed(app.reports[1], outcome.steps[1].plan, signal()),
      { verified: false, diagnostic: { code: "completion_unavailable" } });
    app.application.mountPublish();
    const next = await app.session.submit(command(args));
    assert.equal(next.status, "verified", next.error);
    assert.equal(JSON.parse(next.report).effects, 12);
    assert.equal(Object.hasOwn(JSON.parse(next.report), "execution_failure"), false);
    assert.equal(app.application.snapshot().publishes, 1, "only the separately requested task publishes");
  } finally { workflow.cancel(); app.close(); }
});

test("a worker3 failure completion remains bound to its exact report and is consumed once", async () => {
  const app = fixture();
  app.application.unmountPublish();
  const args = { title: "Retained", content: "Retained" };
  try {
    const catalog = prepareSemanticCatalog(await app.host.describe(signal()), { executionReports: "v1" });
    const plan = catalog.prepare("publish_article", args).plan;
    const first = await app.host.execute(plan, signal());
    assert.deepEqual(await app.host.verifyDetailed(first + " ", plan, signal()),
      { verified: false, diagnostic: { code: "report_mismatch" } });
    assert.deepEqual(await app.host.verifyDetailed(first, plan, signal()),
      { verified: false, diagnostic: { code: "completion_unavailable" } });
    const next = await app.host.execute(plan, signal());
    assert.deepEqual(await app.host.verifyDetailed(next, plan, signal()),
      { verified: false, diagnostic: { code: "execution_object_unavailable" } });
    assert.deepEqual(await app.host.verifyDetailed(next, plan, signal()),
      { verified: false, diagnostic: { code: "completion_unavailable" } });
    assert.equal(app.application.snapshot().publishes, 0);
  } finally { app.close(); }
});

for (const errorText of ["execution_object_unavailable", "unknown registered object"]) {
  test(`a committed callback error '${errorText}' cannot impersonate the typed missing-object diagnostic`, async () => {
    let rewritten = 0, replyStatus;
    const app = fixture({ faultAfterCommit: true }, (reply) => {
      if (reply.status === "ok") return reply;
      rewritten++;
      replyStatus = reply.status;
      // One valid but misleading wire code and one invalid prose error. Neither
      // changes the existing gateway's authority or decorates a result report.
      return { ...reply, error: errorText };
    });
    const args = { title: "Published title", content: "Keep the committed publication" };
    const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
      return prepareSemanticWorkflow(catalog, [
        { task: "publish_article", arguments: args }, { task: "read_content", arguments: {} },
      ]);
    } } });
    try {
      const outcome = await workflow.submit("Publish once and stop on callback failure");
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.error, "application_verification_failed");
      assert.deepEqual(outcome.verificationFailure, { code: "execution_failed" });
      assert.equal(outcome.steps.length, 1);
      assert.equal(rewritten, 1);
      assert.equal(outcome.steps[0].report, app.reports[0]);
      const report = JSON.parse(app.reports[0]);
      assert.equal(report.error, errorText === "execution_object_unavailable"
        ? `JavaScript ${replyStatus}: execution_object_unavailable` : "invalid wire outcome");
      assert.equal(Object.hasOwn(report, "effects"), false, "a failed driver does not invent completed Effect evidence");
      assert.equal(Object.hasOwn(report, "result"), false);
      assert.equal(outcome.possiblePartialEffects, true);
      assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: args.title, savedContent: args.content,
        publishes: 1, otherClicks: 0 });
      const read = await app.session.submit("task read_content");
      assert.equal(read.status, "verified", read.error);
      assert.equal(JSON.parse(read.report).result.value, args.content);
      assert.equal(app.application.snapshot().publishes, 1);
    } finally { workflow.cancel(); app.close(); }
  });
}

test("the opt-in publication composition supports single-string and no-input tasks", async () => {
  const application = createPublicationApplication({ faultAfterCommit: true });
  const app = createPublicationAgent({ application, executionReports: "v1" });
  try {
    const outcome = await app.session.submit("task publish_article:Committed through the public composition ✓");
    assert.equal(outcome.status, "failed");
    assert.deepEqual(outcome.verificationFailure, { code: "execution_failed" });
    assert.equal(outcome.plan.inputMode, undefined, "the old single-string catalog still uses its own admission mode");
    assert.equal(application.snapshot().publishes, 1);
    const read = await app.session.submit("task read_publication");
    assert.equal(read.status, "verified", read.error);
    assert.equal(JSON.parse(read.report).result.value, "Committed through the public composition ✓");
    assert.equal(JSON.parse(read.report).effects, 2);
    assert.equal(application.snapshot().publishes, 1);
  } finally { app.close(); }
});

for (const stop of ["cancel", "revoke"]) {
  test(`worker3 ${stop} fences a late commit and preserves earlier edits`, { timeout: 10_000 }, async () => {
    const entered = deferred(), release = deferred();
    const app = fixture({ async beforePublish() { entered.resolve(); await release.promise; } });
    const args = { title: "Before retirement", content: "Retained edits" };
    try {
      const pending = app.session.submit(command(args));
      await Promise.race([entered.promise, pending.then((outcome) => assert.fail(`action not entered: ${outcome.error}`))]);
      if (stop === "cancel") assert.equal(app.session.cancel(), true);
      else app.bridges.at(-1).revoke();
      const outcome = await pending;
      assert.equal(outcome.status, stop === "cancel" ? "cancelled" : "failed");
      assert.equal(outcome.possiblePartialEffects, true);
      if (stop === "cancel") {
        assert.equal(outcome.report, undefined, "local cancellation does not manufacture a worker report");
        assert.equal(outcome.verificationFailure, undefined);
      }
      release.resolve(); await tick(); await tick();
      assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
      const read = await app.session.submit("task read_title");
      assert.equal(read.status, "verified", read.error);
      assert.equal(JSON.parse(read.report).result.value, args.title);
      assert.equal(app.application.snapshot().publishes, 0);
    } finally { release.resolve(); app.close(); }
  });
}

test("worker3 admission failure remains an exception and a later admitted run uses a fresh process", async () => {
  const carrier = createNodeAgentTransport({ executable: defaultWorker, catalog: (raw) => raw, executionReports: "v1" });
  const endpoint = { window: "admission", epoch: randomUUID() };
  let requests = 0;
  try {
    await assert.rejects(carrier.execute({ scope: { ...endpoint, session: "bad" }, source: "not Lese source",
      arguments: {}, onRequest() { requests++; } }, signal()), /^Error: node_worker_failed:/u);
    assert.equal(requests, 0, "syntax admission precedes every application request");
    const raw = await carrier.execute({ scope: { ...endpoint, session: "good" }, source: "fn main() = true", arguments: {},
      onRequest(request) {
        requests++;
        assert.equal(request.operation, "gui.list");
        void carrier.reply({ transport_version: 1, ...endpoint, session: "good", request_id: request.request_id,
          status: "ok", value: [], error: null }).catch(() => {});
      } }, signal());
    const report = JSON.parse(raw);
    assert.deepEqual(report.result, { type: "boolean", value: true });
    assert.equal(report.effects, 0);
    assert.equal(Object.hasOwn(report, "execution_failure"), false);
  } finally { carrier.close(endpoint); }
});
