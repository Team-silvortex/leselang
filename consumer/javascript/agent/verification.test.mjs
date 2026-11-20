import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createRegisteredApplication,
  createSemanticProvider, defineSemanticCatalog, prepareSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createArticleApplication } from "./multi-input-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

// A host-owned business condition intentionally rejects one completed publication.
function reviewCatalog(base) {
  const { semantics, ...catalog } = JSON.parse(createArticleCatalog(base));
  semantics.tasks.find((task) => task.id === "publish_article").verification.predicates.push({
    id: "requires_review", left: { kind: "path", path: [
      { kind: "field", name: "objects_after" }, { kind: "match", field: "identifier", value: "publish" },
      { kind: "field", name: "clicks" },
    ] }, relation: { kind: "equal" }, right: { kind: "literal", value: { kind: "integer", value: 2 } },
  });
  return defineSemanticCatalog(catalog, semantics);
}

function fixture(application, options = {}) {
  let executions = 0, observations = 0, trace = "";
  const transport = createNodeAgentTransport({ executable: defaultWorker,
    catalog: options.catalog ?? createArticleCatalog, onStderr(chunk) { trace += chunk; } });
  const host = createRegisteredAgentHost({ window: "verification-consumer", epoch: randomUUID(),
    application: { ...application,
      observe() { observations++; return application.observe(); },
      attach(bridge) { application.attach(bridge); options.afterAttach?.(); },
    },
    transport: { describe: (signal) => transport.describe(signal), reply: (reply) => transport.reply(reply),
      cancel: (scope) => transport.cancel(scope), close: (endpoint) => transport.close(endpoint),
      async execute(run, signal) {
        executions++;
        const report = await transport.execute(run, signal);
        options.afterExecute?.();
        return report;
      } }, observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentSession({ host, provider: createSemanticProvider() });
  return { host, session, executions: () => executions, observations: () => observations, trace: () => trace,
    close() { session.cancel(); host.close(); } };
}

test("real worker reports the failed host predicate and stops without retrying a completed publication", async () => {
  const application = createArticleApplication();
  const app = fixture(application, { catalog: reviewCatalog });
  const args = { title: "private title ✓", content: "private content" };
  const events = [];
  const session = new AgentWorkflowSession({ host: app.host, onEvent: (state) => events.push(state),
    provider: { async plan({ catalog }) { return prepareSemanticWorkflow(catalog, [
      { task: "publish_article", arguments: args }, { task: "read_content", arguments: {} },
    ]); } } });
  try {
    const outcome = await session.submit("Publish then read");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.steps.length, 1);
    assert.equal(app.executions(), 1);
    const diagnostic = { code: "predicate_failed", predicate: "requires_review" };
    assert.deepEqual(outcome.verificationFailure, diagnostic);
    assert.deepEqual(outcome.steps[0].verificationFailure, diagnostic);
    assert.deepEqual(events.at(-1).verificationFailure, diagnostic);
    assert.ok(Object.isFrozen(outcome.verificationFailure));
    assert.equal(JSON.stringify(diagnostic).includes(args.title), false);
    const report = JSON.parse(outcome.steps[0].report);
    assert.equal(report.effects, 12);
    assert.equal(report.result.value, true);
    assert.ok(report.fuel_remaining > 0 && report.fuel_remaining < 10_000);
    assert.equal((app.trace().match(/poll -> authorize -> dispatch:/gu) ?? []).length, 12);
    assert.equal((app.trace().match(/^accept$/gmu) ?? []).length, 12);
    assert.equal(app.observations(), 1, "semantic rejection needs no fresh application read");
    assert.deepEqual(application.snapshot(), { ...args, savedTitle: args.title, savedContent: args.content,
      publishes: 1, otherClicks: 0 });
    const read = await app.session.submit("task read_content");
    assert.equal(read.status, "verified", read.error);
    assert.equal(JSON.parse(read.report).result.value, args.content);
    assert.equal(Object.hasOwn(read, "verificationFailure"), false);
    assert.equal(application.snapshot().publishes, 1);
  } finally { session.cancel(); app.close(); }
});

for (const timing of ["before", "after"]) {
  test(`real worker distinguishes ${timing} application observation drift from a valid semantic report`, async () => {
    let title = "initial";
    const application = createRegisteredApplication({ title: { read: () => ({ role: "text_field",
      title: "Article title", value: title, enabled: true, editable: true, clicks: 0 }) } });
    const change = () => { title = "manual edit"; application.changed("title"); };
    const app = fixture(application, timing === "before" ? { afterAttach: change } : { afterExecute: change });
    try {
      const outcome = await app.session.submit("task read_title");
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.error, "application_verification_failed");
      assert.deepEqual(outcome.verificationFailure, { code: `${timing}_observation_mismatch` });
      const catalog = prepareSemanticCatalog(await app.host.describe(new AbortController().signal));
      assert.deepEqual(catalog.prepare("read_title", {}).verifyDetailed(outcome.report), { verified: true });
      assert.equal(JSON.parse(outcome.report).effects, 2);
      assert.equal(app.executions(), 1);
      assert.equal(app.observations(), 2, "one before snapshot and one fresh verification snapshot");
      assert.equal(title, "manual edit", "verification does not restore or overwrite external changes");
    } finally { app.close(); }
  });
}

test("real worker execution faults do not invent verification diagnostics or retry committed effects", async () => {
  const application = createArticleApplication({ faultAfterCommit: true });
  const app = fixture(application);
  try {
    const outcome = await app.session.submit('task publish_article:{"title":"committed","content":"retained"}');
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.report, undefined);
    assert.equal(Object.hasOwn(outcome, "verificationFailure"), false);
    assert.equal(application.snapshot().publishes, 1);
    assert.equal(application.snapshot().savedContent, "retained");
    assert.equal(app.executions(), 1);
  } finally { app.close(); }
});
