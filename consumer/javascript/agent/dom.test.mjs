import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createDomArticleApplication } from "./dom-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

const tick = () => new Promise((resolve) => setImmediate(resolve));
const command = (args) => `task publish_article:${JSON.stringify(args)}`;
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
function fixture(options = {}) {
  const application = createDomArticleApplication(options), bridges = [];
  let trace = "";
  const transport = createNodeAgentTransport({ executable: defaultWorker, catalog: createArticleCatalog,
    onStderr(chunk) { trace += chunk; } });
  const host = createRegisteredAgentHost({ window: "dom-consumer", epoch: randomUUID(),
    application: { ...application, attach(bridge) { bridges.push(bridge); application.attach(bridge); } }, transport,
    observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentSession({ host, provider: createSemanticProvider() });
  return { application, host, session, bridges, trace: () => trace,
    close() { session.cancel(); host.close(); application.dispose(); } };
}

test("public DOM helpers route EventTarget handlers through a real seventeen-Effect bound workflow", async () => {
  const app = fixture();
  const initial = { title: 'Event title "✓"', content: "Independent DOM content\nsecond line" };
  app.application.manualInput("title", initial.title);
  app.application.manualInput("content", initial.content);
  const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} }, { task: "read_content", arguments: {} },
      { task: "publish_article", arguments: {
        title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" },
      } }, { task: "check_article", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await workflow.submit("Read both fields, publish their verified values, then check");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(outcome.steps.map((step) => JSON.parse(step.report).effects), [2, 2, 12, 1]);
    assert.deepEqual({ ...outcome.steps[2].plan.arguments }, initial);
    assert.deepEqual(app.application.snapshot(), { ...initial, savedTitle: initial.title, savedContent: initial.content,
      publishes: 1, otherClicks: 0 });
    assert.deepEqual(app.application.eventCounts(), { title: 2, content: 2, publish: 1, other: 0 });
    assert.equal(app.application.elements.savedTitle.textContent, initial.title);
    assert.equal(app.application.elements.savedContent.textContent, initial.content);
    assert.equal((app.trace().match(/poll -> authorize -> dispatch:/gu) ?? []).length, 17);
    assert.equal((app.trace().match(/^accept$/gmu) ?? []).length, 17);
    app.application.manualClick("other");
    assert.equal(app.application.snapshot().otherClicks, 1, "manual interaction remains available after session teardown");
  } finally { workflow.cancel(); app.close(); }
});

for (const stop of ["cancel", "revoke"]) {
  test(`DOM ${stop} fences a late event-handler commit and retains the two earlier edits`, async () => {
    const entered = deferred(), release = deferred();
    const app = fixture({ async beforePublish() { entered.resolve(); await release.promise; } });
    const args = { title: "retained title", content: "retained content" };
    try {
      const pending = app.session.submit(command(args));
      await Promise.race([entered.promise, pending.then((outcome) => assert.fail(`publication not entered: ${outcome.error}`))]);
      if (stop === "cancel") app.session.cancel(); else app.bridges.at(-1).revoke();
      const outcome = await pending;
      assert.equal(outcome.status, stop === "cancel" ? "cancelled" : "failed");
      assert.equal(outcome.possiblePartialEffects, true);
      release.resolve(); await tick(); await tick();
      assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
      assert.deepEqual(app.application.eventCounts(), { title: 1, content: 1, publish: 1, other: 0 });
      const read = await app.session.submit("task read_content");
      assert.equal(read.status, "verified", read.error);
      assert.equal(JSON.parse(read.report).result.value, args.content);
    } finally { release.resolve(); app.close(); }
  });
}

test("a DOM handler fault after commit preserves publication without retrying or inventing verification", async () => {
  const app = fixture({ faultAfterCommit: true });
  const args = { title: "committed title", content: "committed content" };
  try {
    const outcome = await app.session.submit(command(args));
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.report, undefined);
    assert.equal(Object.hasOwn(outcome, "verificationFailure"), false);
    assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: args.title, savedContent: args.content,
      publishes: 1, otherClicks: 0 });
    assert.equal(app.application.eventCounts().publish, 1);
    const read = await app.session.submit("task read_title");
    assert.equal(read.status, "verified", read.error);
    assert.equal(app.application.snapshot().publishes, 1);
  } finally { app.close(); }
});

test("replacing a DOM button during an awaited handler blocks its late publication", async () => {
  const entered = deferred(), release = deferred();
  const app = fixture({ async beforePublish() { entered.resolve(); await release.promise; } });
  try {
    const pending = app.session.submit(command({ title: "retained title", content: "retained content" }));
    await Promise.race([entered.promise, pending.then((outcome) => assert.fail(`publication not entered: ${outcome.error}`))]);
    app.application.replaceNode("publish");
    release.resolve();
    const outcome = await pending;
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(app.application.snapshot().publishes, 0);
    assert.equal(app.application.snapshot().savedContent, "");
    assert.throws(() => app.application.observe());
  } finally { release.resolve(); app.close(); }
});
