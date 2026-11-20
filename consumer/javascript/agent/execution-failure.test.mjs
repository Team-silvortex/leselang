import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  prepareSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createDynamicDomArticleApplication } from "./dom-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

// The VM and carrier are real. The optional report decoration below is an
// explicitly injected terminal failure, not a claim about native classification.
function fixture(decorate = (report) => report) {
  const application = createDynamicDomArticleApplication({ title: "Observed draft", content: "Retained content" });
  const carrier = createNodeAgentTransport({ executable: defaultWorker, catalog: createArticleCatalog });
  const reports = [];
  let catalog, executions = 0;
  const host = createRegisteredAgentHost({ window: "failure-consumer", epoch: randomUUID(), application,
    executionReports: "v1", observations: { before: "objects_before", after: "objects_after" },
    transport: {
      async describe(signal) { catalog = await carrier.describe(signal); return catalog; },
      reply: (reply) => carrier.reply(reply), cancel: (scope) => carrier.cancel(scope), close: (endpoint) => carrier.close(endpoint),
      async execute(run, signal) {
        executions++;
        const report = decorate(await carrier.execute(run, signal));
        reports.push(report); return report;
      },
    } });
  const session = new AgentSession({ host, provider: createSemanticProvider() });
  return { application, host, session, reports, catalog: () => catalog, executions: () => executions,
    close() { session.cancel(); host.close(); application.dispose(); } };
}

function failed(report) {
  return JSON.stringify({ ...JSON.parse(report), status: "failed", error: "injected failure after worker completion",
    possible_partial_effects: false, execution_failure: { code: "execution_failed" } });
}

test("a real source result marked failed cannot supply a workflow binding", async () => {
  let inject = true;
  const app = fixture((report) => inject ? failed(report) : report);
  const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} },
      { task: "publish_article", arguments: { title: { fromStep: 0, format: "string" }, content: "Never published" } },
    ]);
  } } });
  try {
    const outcome = await workflow.submit("Do not use a failed source even when its scalar is correct");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.deepEqual(outcome.verificationFailure, { code: "execution_failed" });
    assert.deepEqual(outcome.steps[0].verificationFailure, outcome.verificationFailure);
    assert.equal(outcome.steps.length, 1);
    assert.equal(app.executions(), 1);
    assert.equal(outcome.steps[0].report, app.reports[0]);
    assert.equal(JSON.parse(outcome.steps[0].report).result.value, "Observed draft");
    assert.equal(outcome.possiblePartialEffects, true, "untrusted report cannot clear the session's conservative flag");
    assert.deepEqual(app.application.eventCounts(), { title: 0, content: 0, publish: 0, other: 0 });
    inject = false;
    const next = await app.session.submit("task read_title");
    assert.equal(next.status, "verified", next.error);
    assert.equal(app.application.snapshot().publishes, 0);
  } finally { workflow.cancel(); app.close(); }
});

test("an explicit terminal failure preserves a real publication and all original success evidence", async () => {
  let inject = true;
  const app = fixture((report) => inject ? failed(report) : report);
  const args = { title: "Already published", content: "Keep the effect, refuse success" };
  const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "publish_article", arguments: args }, { task: "read_content", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await workflow.submit("Retain a publication whose final outcome is failed");
    assert.equal(outcome.status, "failed");
    assert.deepEqual(outcome.verificationFailure, { code: "execution_failed" });
    assert.equal(outcome.steps.length, 1);
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.steps[0].report, app.reports[0]);
    assert.equal(JSON.parse(outcome.steps[0].report).effects, 12);
    assert.equal(app.application.snapshot().publishes, 1);
    assert.equal(app.application.snapshot().savedContent, args.content);
    assert.equal(prepareSemanticCatalog(app.catalog()).prepare("publish_article", args).verify(app.reports[0]), true,
      "the old generic contract keeps unrelated business fields uninterpreted");
    assert.equal(prepareSemanticCatalog(app.catalog(), { executionReports: "v1" })
      .prepare("publish_article", args).verify(app.reports[0]), false);
    assert.deepEqual(await app.host.verifyDetailed(app.reports[0], outcome.steps[0].plan, new AbortController().signal),
      { verified: false, diagnostic: { code: "completion_unavailable" } });
    inject = false;
    const next = await app.session.submit("task read_content");
    assert.equal(next.status, "verified", next.error);
    assert.equal(app.application.snapshot().publishes, 1, "a later read does not retry the earlier effect");
  } finally { workflow.cancel(); app.close(); }
});

test("legacy Node worker errors retain their envelope and partial edits with report opt-in", async () => {
  const app = fixture();
  const args = { title: "Retained title", content: "Retained edit before missing button" };
  app.application.unmountPublish();
  try {
    const outcome = await app.session.submit(`task publish_article:${JSON.stringify(args)}`);
    assert.equal(outcome.status, "failed");
    assert.match(outcome.error, /^node_worker_failed: unknown registered object$/u);
    assert.equal(outcome.report, undefined);
    assert.equal(outcome.verificationFailure, undefined, "a transport exception is not invented completion evidence");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
    assert.deepEqual(app.application.eventCounts(), { title: 1, content: 1, publish: 0, other: 0 });
    app.application.mountPublish();
    const next = await app.session.submit(`task publish_article:${JSON.stringify(args)}`);
    assert.equal(next.status, "verified", next.error);
    assert.equal(app.application.snapshot().publishes, 1);
  } finally { app.close(); }
});
