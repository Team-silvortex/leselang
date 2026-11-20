// Real independent application -> Rust worker -> public Bridge acceptance.
// Build leselang-agent-worker and the compatibility example before this suite.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { AgentSession, AgentWorkflowSession, createRegisteredAgentHost, createSemanticProvider,
  defineSemanticCatalog, prepareSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createPublicationApplication } from "./application.mjs";
import { createPublicationAgent, defaultWorker } from "./assistant.mjs";
import { createPublicationAgentCatalog } from "./catalog.mjs";
import { createNodeAgentTransport } from "@leselang/bridge/node";

function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));
const report = (step) => JSON.parse(step.report);

test("an authored u64 literal survives catalog serialization, generated source and the real VM report", async () => {
  const maximum = 18_446_744_073_709_551_615n;
  const description = {
    version: 1,
    operations: [{ operation: "gui.list", summary: "Inspect the registered inventory.", effect: "read", completion: "value" }],
    tasks: [{
      id: "exact_integer", summary: "Return an exact declared integer and verify the inventory is unchanged.",
      inputs: [], execution: "no_text",
      steps: [{ kind: "call", binding: "ids", operation: "gui.list", arguments: [] }],
      result: { kind: "literal", value: { kind: "integer", value: maximum } },
      verification: { kind: "javascript_task_run", max_fuel: 10_000,
        result: { kind: "integer", value: maximum }, require_verified: false,
        predicates: [{ id: "unchanged", left: { kind: "path", path: [{ kind: "field", name: "objects_before" }] },
          relation: { kind: "equal" }, right: { kind: "path", path: [{ kind: "field", name: "objects_after" }] } }],
      },
    }],
  };
  let catalogRaw;
  const transport = createNodeAgentTransport({ executable: defaultWorker, catalog(base) {
    catalogRaw = defineSemanticCatalog(base, description);
    return catalogRaw;
  } });
  const host = createRegisteredAgentHost({ window: "authored-integer", epoch: randomUUID(), transport,
    application: { attach() {}, detach() {}, observe() { return []; } },
    observations: { before: "objects_before", after: "objects_after" },
  });
  const session = new AgentSession({ host, provider: createSemanticProvider() });
  try {
    const outcome = await session.submit("task exact_integer");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(outcome.possiblePartialEffects, false);
    assert.ok(catalogRaw.includes('"value":18446744073709551615'));
    assert.ok(outcome.plan.source.includes("18446744073709551615"));
    assert.ok(outcome.report.includes('"value":18446744073709551615'));
    const prepared = prepareSemanticCatalog(catalogRaw).prepare("exact_integer", {});
    assert.equal(prepared.expectedEffects, 1);
    assert.equal(prepared.verify(outcome.report), true);
    assert.equal(prepared.verify(outcome.report.replace('"value":18446744073709551615', '"value":18446744073709551614')), false);
  } finally { session.cancel(); host.close(); }
});
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

test("the Node-owned catalog discovers all four tasks from the actual offline worker", () => {
  const native = spawnSync(defaultWorker, ["--describe"], { encoding: "utf8", timeout: 5_000 });
  assert.ifError(native.error);
  assert.equal(native.status, 0, native.stderr);
  assert.equal(native.stderr, "");
  const catalog = prepareSemanticCatalog(createPublicationAgentCatalog(native.stdout));
  assert.equal(catalog.profile, "javascript.registered");
  assert.deepEqual(catalog.tasks.map(({ id }) => id), ["read_draft", "publish_article", "read_publication", "check_publication"]);
  assert.deepEqual(catalog.tasks.map(({ id }) => catalog.prepare(id, id === "publish_article" ? { text: "x" } : {}).expectedEffects), [2, 9, 2, 1]);
});

test("the standalone worker and legacy example expose identical offline catalog bytes", () => {
  const legacy = fileURLToPath(new URL("../../../target/lean/examples/agent_worker", import.meta.url));
  const results = [defaultWorker, legacy].map((executable) => {
    const result = spawnSync(executable, ["--describe"], { encoding: "utf8", timeout: 5_000 });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stderr, "");
    return result.stdout;
  });
  assert.equal(results[0], results[1]);
});

test("one live JavaScript application executes a bound conditional workflow through fresh Rust VMs", { timeout: 20_000 }, async () => {
  const text = 'Node "发布" \\ path\nsecond line ✓';
  const application = createPublicationApplication({ initialDraft: text });
  let trace = "";
  const app = createPublicationAgent({ application, onStderr: (chunk) => { trace += chunk; } });
  const session = workflow(app.host, text);
  try {
    const outcome = await session.submit("run the declared workflow");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(outcome.possiblePartialEffects, false);
    assert.deepEqual(outcome.steps.map((step) => report(step).effects), [2, 9, 2, 1]);
    assert.deepEqual(outcome.steps.map((step) => report(step).result), [
      { type: "string", value: text }, { type: "boolean", value: true },
      { type: "string", value: text }, { type: "none", value: null },
    ]);
    assert.equal(outcome.requirements[0].satisfied, true);
    assert.equal(outcome.requirements[0].actual, text);
    for (const step of outcome.steps) {
      const value = report(step);
      assert.equal(value.kind, "javascript_task_run");
      assert.equal(value.profile, "javascript.registered");
      assert.ok(value.fuel_remaining > 0 && value.fuel_remaining < 10_000);
      assert.equal(Object.hasOwn(value, "verified"), false, "business verification belongs to the application");
    }
    assert.equal(outcome.steps[1].plan.arguments.text, text);
    assert.ok(!outcome.steps[1].plan.source.includes(text), "input remains data");
    assert.equal((trace.match(/poll -> authorize -> dispatch:/gu) ?? []).length, 14);
    assert.equal((trace.match(/^accept$/gmu) ?? []).length, 14);
    assert.deepEqual(application.snapshot(), { draft: text, published: text, status: "published", publishes: 1, resets: 0, unrelated: 0 });

    const nextText = "--fault-after-commit";
    const next = await app.session.submit(`task publish_article:${nextText}`);
    assert.equal(next.status, "verified", next.error);
    assert.equal(report(next).objects_before.find((row) => row.identifier === "publish").clicks, 1);
    assert.equal(application.snapshot().published, nextText);
    assert.equal(application.snapshot().publishes, 2, "new worker observes the same application, not a reset model");
    assert.equal(application.snapshot().unrelated, 0);
    const read = await app.session.submit("task read_publication");
    assert.equal(read.status, "verified", read.error);
    assert.equal(report(read).result.value, nextText);
  } finally { session.cancel(); app.close(); }
});

test("a real observed result that fails a condition never dispatches the publication", async () => {
  const application = createPublicationApplication({ initialDraft: "actual current draft" });
  let trace = "";
  const app = createPublicationAgent({ application, onStderr: (chunk) => { trace += chunk; } });
  const session = workflow(app.host, "required other draft");
  try {
    const outcome = await session.submit("conditional publication");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "workflow_requirement_not_met");
    assert.equal(outcome.steps.length, 2, "the target records refusal during planning without entering execution");
    assert.equal(outcome.steps[0].status, "verified");
    assert.equal(outcome.steps[1].possiblePartialEffects, false);
    assert.equal(outcome.steps[1].report, undefined);
    assert.equal(outcome.requirements[0].actual, "actual current draft");
    assert.equal(outcome.requirements[0].satisfied, false);
    assert.equal(application.snapshot().publishes, 0);
    assert.equal(application.snapshot().published, "");
    assert.equal((trace.match(/poll -> authorize -> dispatch:/gu) ?? []).length, 2);
  } finally { session.cancel(); app.close(); }
});

test("a post-commit application failure survives worker failure and stops the workflow", async () => {
  const text = "publication already happened ✓";
  const application = createPublicationApplication({ initialDraft: text, faultAfterCommit: true });
  const app = createPublicationAgent({ application });
  const session = workflow(app.host, text);
  try {
    const outcome = await session.submit("publish then verify");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.steps.length, 2);
    assert.equal(outcome.steps[0].status, "verified");
    assert.equal(outcome.steps[1].status, "failed");
    assert.equal(outcome.steps[1].report, undefined);
    assert.deepEqual(application.snapshot(), { draft: text, published: text, status: "published", publishes: 1, resets: 0, unrelated: 0 });
    const observed = await app.session.submit("task read_publication");
    assert.equal(observed.status, "verified", observed.error);
    assert.equal(report(observed).result.value, text);
    assert.equal(application.snapshot().publishes, 1, "failure does not retry the action");
  } finally { session.cancel(); app.close(); }
});

test("cancelling a deferred real-worker action fences its late commit before another task", { timeout: 10_000 }, async () => {
  const entered = deferred(), release = deferred();
  const application = createPublicationApplication({ initialDraft: "before", async beforePublish() {
    entered.resolve(); await release.promise;
  } });
  const app = createPublicationAgent({ application });
  try {
    const pending = app.session.submit("task publish_article:retained draft only");
    await Promise.race([entered.promise, pending.then((result) => assert.fail(`did not enter action: ${result.error}`))]);
    assert.equal(app.session.cancel(), true);
    const outcome = await pending;
    assert.equal(outcome.status, "cancelled");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(application.snapshot().draft, "retained draft only", "earlier committed set_text survives cancellation");
    assert.equal(application.snapshot().publishes, 0);
    release.resolve(); await tick(); await tick();
    assert.equal(application.snapshot().publishes, 0, "late application Promise cannot commit");
    const read = await app.session.submit("task read_draft");
    assert.equal(read.status, "verified", read.error);
    assert.equal(report(read).result.value, "retained draft only");
  } finally { release.resolve(); app.close(); }
});

test("a real successful VM report is refused when fresh application evidence differs", async () => {
  const model = createPublicationApplication();
  let observeCalls = 0;
  const application = { ...model, observe() {
    const rows = model.observe();
    if (++observeCalls === 2) rows.find((row) => row.identifier === "publish").value = "different current observation";
    return rows;
  } };
  const app = createPublicationAgent({ application });
  try {
    const outcome = await app.session.submit("task publish_article:real publication");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(report(outcome).result.value, true);
    assert.equal(model.snapshot().published, "real publication");
    assert.equal(model.snapshot().publishes, 1);
  } finally { app.close(); }
});

test("an undeclared task never starts an execution worker or touches application state", async () => {
  const application = createPublicationApplication();
  let trace = "";
  const app = createPublicationAgent({ application, onStderr: (chunk) => { trace += chunk; } });
  try {
    const outcome = await app.session.submit("task save_message:wrong application");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, false);
    assert.equal(trace, "");
    assert.equal(application.snapshot().publishes, 0);
  } finally { app.close(); }
});

test("the actual worker never requests controls for inadmissible or closed startup input", () => {
  const start = { agent_worker_version: 1, kind: "start", scope: { window: "test", epoch: "fresh", session: "one" },
    source: "fn main() = true", arguments: {} };
  const invalid = [
    JSON.stringify({ ...start, source: "fn main() = missing_operation()" }) + "\n",
    JSON.stringify({ ...start, arguments: { unbound: "value" } }) + "\n",
    JSON.stringify({ ...start, profile: "tauri.typescript" }) + "\n",
    JSON.stringify({ ...start, scope: { ...start.scope, session: "" } }) + "\n",
    JSON.stringify(start).replace('"kind":"start"', '"kind":"reply","kind":"start"') + "\n",
    JSON.stringify({ ...start, source: " ".repeat(16385) }) + "\n",
    JSON.stringify(start),
    Buffer.from([0xff, 0x0a]),
  ];
  for (const input of invalid) {
    const result = spawnSync(defaultWorker, ["--serve"], { input, encoding: "utf8", timeout: 7_000, maxBuffer: 512 * 1024 });
    assert.ifError(result.error);
    assert.equal(result.status, 1, result.stderr);
    const records = result.stdout.trim().split("\n").filter(Boolean).map((line) => JSON.parse(line));
    assert.equal(records.length, 1, result.stdout);
    assert.equal(records[0].kind, "error", result.stdout);
  }
});

test("the real worker compiles before exchanging requests and preserves exact u64 report bytes", async () => {
  const transport = createNodeAgentTransport({ executable: defaultWorker, catalog: createPublicationAgentCatalog });
  const endpoint = { window: "direct-test", epoch: "new-instance" };
  let requests = 0;
  try {
    for (const [index, source, arguments_] of [
      [1, "fn main() = missing_operation()", {}],
      [2, "fn main() = true", { unbound: "value" }],
    ]) {
      await assert.rejects(transport.execute({ scope: { ...endpoint, session: `invalid_${index}` }, source, arguments: arguments_,
        onRequest() { requests++; assert.fail("invalid source or arguments reached the application"); },
      }, new AbortController().signal), /node_worker_failed/);
    }
    assert.equal(requests, 0);
    const raw = await transport.execute({ scope: { ...endpoint, session: "integer" },
      source: "fn main() = 18446744073709551615", arguments: {},
      onRequest(request) {
        requests++;
        assert.equal(request.operation, "gui.list");
        void transport.reply({ transport_version: 1, ...endpoint, session: "integer", request_id: request.request_id,
          status: "ok", value: [], error: null }).catch(() => {});
      },
    }, new AbortController().signal);
    assert.match(raw, /"value":18446744073709551615/);
    assert.equal(requests, 1, "the pure task only needs the host's empty-registry bootstrap");
  } finally { transport.close(endpoint); }
});

test("the actual worker bounds both missing startup and missing Bridge replies", { timeout: 12_000 }, async () => {
  await Promise.all([false, true].map((sendStart) => new Promise((resolve, reject) => {
    const child = spawn(defaultWorker, ["--serve"], { stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "", stderr = "", failure;
    const began = performance.now();
    const timer = setTimeout(() => {
      failure = new Error("worker exceeded its own I/O deadline"); child.kill("SIGKILL");
    }, 9_000);
    child.on("error", (error) => { failure = error; });
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.stdin.on("error", () => {});
    child.on("close", (code) => {
      clearTimeout(timer);
      try {
        if (failure) throw failure;
        assert.equal(code, 1, stderr);
        const frames = stdout.trim().split("\n").filter(Boolean).map((line) => JSON.parse(line));
        assert.equal(frames.at(-1).kind, "error", stdout);
        assert.match(frames.at(-1).error, /deadline/);
        assert.equal(frames.filter(({ kind }) => kind === "request").length, sendStart ? 1 : 0);
        assert.ok(performance.now() - began < 8_000);
        resolve();
      } catch (error) { reject(error); }
    });
    if (sendStart) child.stdin.write(JSON.stringify({ agent_worker_version: 1, kind: "start",
      scope: { window: "deadline", epoch: "instance", session: "one" }, source: "fn main() = true", arguments: {} }) + "\n");
  })));
});
