import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  createModelProvider, prepareSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createArticleApplication } from "./multi-input-application.mjs";
import { createArticleCatalog } from "./multi-input-catalog.mjs";
import { defaultWorker } from "./assistant.mjs";

function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));
const report = (outcome) => JSON.parse(outcome.report);
const command = (args) => `task publish_article:${JSON.stringify(args)}`;
function fixture(options = {}, provider = createSemanticProvider()) {
  const application = createArticleApplication(options);
  const runs = [], bridges = [], requests = [];
  let trace = "", catalog;
  const carrier = createNodeAgentTransport({ executable: defaultWorker, catalog: createArticleCatalog,
    onStderr(value) { trace += value; } });
  const host = createRegisteredAgentHost({ window: "multi-input-article", epoch: randomUUID(),
    application: { ...application, attach(bridge) { bridges.push(bridge); application.attach(bridge); } },
    transport: { async describe(signal) { catalog = await carrier.describe(signal); return catalog; }, reply: (reply) => carrier.reply(reply),
      cancel: (scope) => carrier.cancel(scope), close: (endpoint) => carrier.close(endpoint),
      execute(run, signal) {
        runs.push(run);
        return carrier.execute({ ...run, onRequest(request) { requests.push(request); run.onRequest(request); } }, signal);
      } }, observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentSession({ host, provider });
  return { application, host, session, runs, bridges, requests, trace: () => trace, catalog: () => catalog,
    close() { session.cancel(); host.close(); } };
}

test("v3 publishes two literal inputs through one ordinary twelve-Effect program", async () => {
  const args = { title: '标题 "quoted" ✓', content: '第一段\\path\n{ "task": "other" }' };
  const app = fixture();
  try {
    const outcome = await app.session.submit(command(args));
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(outcome.plan.inputMode, "named_strings");
    assert.deepEqual({ ...outcome.plan.arguments }, args);
    assert.equal(outcome.plan.source.includes(args.title), false);
    assert.equal(outcome.plan.source.includes(args.content), false);
    assert.equal(report(outcome).effects, 12);
    assert.ok(report(outcome).fuel_remaining > 0 && report(outcome).fuel_remaining < 10_000);
    assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: args.title, savedContent: args.content,
      publishes: 1, otherClicks: 0 });
    assert.equal((app.trace().match(/poll -> authorize -> dispatch:/gu) ?? []).length, 12);
    assert.equal((app.trace().match(/^accept$/gmu) ?? []).length, 12);
    assert.equal(app.runs[0].inputMode, "named_strings");
    const prepared = prepareSemanticCatalog(app.catalog()).prepare("publish_article", args);
    const tampered = report(outcome);
    tampered.objects_after.find((row) => row.identifier === "publish").value = args.title;
    assert.equal(prepared.verify(JSON.stringify(tampered)), false, "content has its own input predicate");
    assert.equal(prepareSemanticCatalog(app.catalog()).prepare("publish_article", {
      title: args.content, content: args.title,
    }).verify(outcome.report), false, "names cannot exchange equally typed arguments");
  } finally { app.close(); }
});

test("two verified observations bind independently to a later v3 task", async () => {
  const initial = { title: "Read this title", content: "Read this different content ✓" };
  const app = fixture(initial);
  const session = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_title", arguments: {} },
      { task: "read_content", arguments: {} },
      { task: "publish_article", arguments: {
        title: { fromStep: 0, format: "string" }, content: { fromStep: 1, format: "string" },
      }, requires: { fromStep: 0, format: "string", equals: initial.title } },
      { task: "check_article", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await session.submit("Read both fields, publish them and verify");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(outcome.steps.map((step) => report(step).effects), [2, 2, 12, 1]);
    assert.deepEqual({ ...outcome.steps[2].plan.arguments }, initial);
    assert.ok(app.runs.every((run) => run.inputMode === "named_strings"));
    assert.equal(app.application.snapshot().savedContent, initial.content);
    assert.equal(app.application.snapshot().savedTitle, initial.title);
    assert.equal(app.application.snapshot().publishes, 1);
  } finally { session.cancel(); app.close(); }
});

test("a local model-provider fixture selects named inputs without generating source or using credits", async () => {
  const args = { title: "Model-selected literal title", content: "Literal content from the request" };
  let calls = 0;
  const app = fixture({}, createModelProvider({ async complete(request) {
    calls++;
    const context = JSON.parse(request.user);
    assert.equal(JSON.parse(context.catalog).semantics.version, 3);
    assert.deepEqual(context.tasks.find((task) => task.id === "publish_article").inputNames, ["title", "content"]);
    return JSON.stringify({ task: "publish_article", arguments: args });
  } }));
  try {
    const outcome = await app.session.submit(`Publish title ${args.title} and content ${args.content}`);
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(calls, 1);
    assert.equal(report(outcome).effects, 12);
  } finally { app.close(); }
});

test("invalid named inputs fail before any application attachment or execution", async () => {
  for (const message of [
    command({ title: "missing content" }), command({ title: "x", content: "y", extra: "z" }),
    command({ title: "x", content: 42 }), command({ title: "x", content: "a".repeat(4097) }),
    'task publish_article:{"title":"first","title":"second","content":"x"}',
    'task publish_article:plain text is not a named argument map',
  ]) {
    const app = fixture();
    try {
      const outcome = await app.session.submit(message);
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.possiblePartialEffects, false);
      assert.equal(app.runs.length, 0);
      assert.equal(app.bridges.length, 0);
      assert.equal(app.application.snapshot().publishes, 0);
    } finally { app.close(); }
  }
});

test("a v3 plan cannot be downgraded by stripping its explicit input mode", async () => {
  const app = fixture();
  const signal = new AbortController().signal;
  try {
    const catalog = prepareSemanticCatalog(await app.host.describe(signal));
    const { inputMode: _, ...downgraded } = catalog.prepare("read_title", {}).plan;
    await assert.rejects(app.host.execute(downgraded, signal));
    assert.equal(app.runs.length, 0);
    assert.equal(app.bridges.length, 0);
  } finally { app.close(); }
});

test("a second-field failure keeps the first mutation and never publishes or retries", async () => {
  const app = fixture({ title: "old", content: "old content", rejectContent: true });
  try {
    const outcome = await app.session.submit(command({ title: "changed title", content: "rejected content" }));
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(app.application.snapshot(), { title: "changed title", content: "old content",
      savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
    assert.equal(app.requests.some((request) => request.operation === "gui.click"), false);
  } finally { app.close(); }
});

for (const stop of ["cancel", "revoke"]) {
  test(`${stop} fences the late publication while retaining both earlier field edits`, async () => {
    const entered = deferred(), release = deferred();
    const args = { title: "retained title", content: "retained content" };
    const app = fixture({ async beforePublish() { entered.resolve(); await release.promise; } });
    try {
      const pending = app.session.submit(command(args));
      await Promise.race([entered.promise, pending.then((outcome) => assert.fail(`publication not entered: ${outcome.error}`))]);
      if (stop === "cancel") app.session.cancel();
      else app.bridges.at(-1).revoke();
      const outcome = await pending;
      assert.equal(outcome.status, stop === "cancel" ? "cancelled" : "failed");
      assert.equal(outcome.possiblePartialEffects, true);
      release.resolve(); await tick(); await tick();
      assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
      const read = await app.session.submit("task read_content");
      assert.equal(read.status, "verified", read.error);
      assert.equal(report(read).result.value, args.content);
    } finally { release.resolve(); app.close(); }
  });
}

test("a failure after publication preserves both saved fields without a retry", async () => {
  const args = { title: "Committed title", content: "Committed content" };
  const app = fixture({ faultAfterCommit: true });
  try {
    const outcome = await app.session.submit(command(args));
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(app.application.snapshot(), { ...args, savedTitle: args.title,
      savedContent: args.content, publishes: 1, otherClicks: 0 });
    assert.equal(app.requests.filter((request) => request.operation === "gui.click").length, 1);
    const read = await app.session.submit("task read_content");
    assert.equal(read.status, "verified", read.error);
    assert.equal(report(read).result.value, args.content);
    assert.equal(app.application.snapshot().publishes, 1);
  } finally { app.close(); }
});

test("multi-input declarations and source match the independent Rust Consumer fixtures", async () => {
  const base = spawnSync(defaultWorker, ["--describe"], { encoding: "utf8", timeout: 5000 });
  assert.ifError(base.error); assert.equal(base.status, 0, base.stderr);
  const raw = createArticleCatalog(base.stdout);
  const fixture = (name) => readFile(new URL(`../../../fixtures/host-semantics/${name}`, import.meta.url), "utf8");
  assert.equal(raw + "\n", await fixture("multi-input-semantics-v3.json"));
  const plan = prepareSemanticCatalog(raw).prepare("publish_article", { title: "literal title", content: "literal content" }).plan;
  assert.equal(plan.source, await fixture("multi-input-save.lese"));
});

test("the declared-task CLI carries separate named arguments and refuses missing values", () => {
  const cli = fileURLToPath(new URL("./multi-input-cli.mjs", import.meta.url));
  const source = fileURLToPath(new URL("../../../fixtures/host-semantics/multi-input-save.lese", import.meta.url));
  const args = { title: 'CLI "title" ✓', content: "Independent CLI content\nsecond line" };
  const run = spawnSync(process.execPath, [cli, "--run", source, "--entry", "main",
    "--arg", "title", args.title, "--arg", "content", args.content, "--report-json"],
  { encoding: "utf8", timeout: 15_000 });
  assert.ifError(run.error);
  assert.equal(run.status, 0, run.stderr);
  const evidence = JSON.parse(run.stdout);
  assert.equal(evidence.effects, 12);
  const saved = evidence.objects_after.find((row) => row.identifier === "publish");
  assert.equal(saved.title, args.title);
  assert.equal(saved.value, args.content);
  const rejected = spawnSync(process.execPath, [cli, "--run", source, "--entry", "main",
    "--arg", "title", args.title, "--report-json"], { encoding: "utf8", timeout: 15_000 });
  assert.ifError(rejected.error);
  assert.equal(rejected.status, 1);
  assert.equal(rejected.stdout, "");
});
