// Public task construction exercised by a different application and the real VM.
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import test from "node:test";
import { AgentSession, AgentWorkflowSession, createRegisteredApplication, createRegisteredAgentHost,
  createSemanticProvider, defineRegisteredTask, defineSemanticCatalog, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { defaultWorker } from "./assistant.mjs";

const provider = createSemanticProvider();
const literal = (kind, value) => ({ kind: "literal", value: { kind, value } });
const path = (phase, control, field) => [
  { kind: "field", name: phase }, { kind: "match", field: "identifier", value: control },
  ...(field ? [{ kind: "field", name: field }] : []),
];
const scalar = (phase, control, field, scalar_type) => ({ kind: "scalar", scalar_type, path: path(phase, control, field) });
const equal = (id, left, right) => ({ id, left, relation: { kind: "equal" }, right });
const unchanged = (control) => equal(`${control}_unchanged`, { kind: "path", path: path("objects_before", control) },
  { kind: "path", path: path("objects_after", control) });
const report = (result, predicates) => ({ kind: "javascript_task_run", max_fuel: 10_000,
  require_verified: false, result, predicates });

function catalog(base) {
  return defineSemanticCatalog(base, {
    version: 2,
    operations: ["gui.find", "gui.observe", "gui.click", "gui.set_text"].map((operation) => ({ operation,
      summary: `Application registered operation ${operation}.`,
      effect: ["gui.click", "gui.set_text"].includes(operation) ? "write" : "read",
      completion: ["gui.click", "gui.set_text"].includes(operation) ? "submission" : "value" })),
    tasks: [
      defineRegisteredTask({
        id: "read_review", summary: "Read the review status without changing the board.",
        inputs: [], execution: "no_text", failure: { kind: "string", value: "" },
        steps: [{ kind: "read_control", control: "review", binding: "observed" }],
        result: { kind: "field", binding: "observed", name: "value" },
        verification: report({ kind: "observed", scalar_type: "string", path: path("objects_after", "review", "value") },
          [unchanged("review"), unchanged("note")]),
      }),
      defineRegisteredTask({
        id: "complete_review", summary: "Complete the review once, disable it and preserve the note.",
        inputs: [], execution: "no_text", failure: { kind: "boolean", value: false },
        steps: [{ kind: "click_control", control: "review", binding: "completed" }],
        result: literal("boolean", true),
        verification: report({ kind: "boolean", value: true }, [
          equal("done", scalar("objects_after", "review", "value", "string"), literal("string", "done")),
          equal("disabled", scalar("objects_after", "review", "enabled", "boolean"), literal("boolean", false)),
          { id: "one_completion", left: scalar("objects_before", "review", "clicks", "integer"),
            relation: { kind: "increased_by", amount: 1 }, right: scalar("objects_after", "review", "clicks", "integer") },
          unchanged("note"),
        ]),
      }),
      defineRegisteredTask({
        id: "write_locked_note", summary: "Write the note once and verify it becomes read-only and disabled.",
        inputs: [{ name: "content", scalar_type: "string" }], execution: "text", failure: { kind: "string", value: "" },
        steps: [{ kind: "set_control_text", control: "note", binding: "written", text: { kind: "input", name: "content" } }],
        result: { kind: "field", binding: "written", name: "value" },
        verification: report({ kind: "observed", scalar_type: "string", path: path("objects_after", "note", "value") }, [
          equal("text", scalar("objects_after", "note", "value", "string"), { kind: "input", name: "content" }),
          equal("disabled", scalar("objects_after", "note", "enabled", "boolean"), literal("boolean", false)),
          equal("readonly", scalar("objects_after", "note", "editable", "boolean"), literal("boolean", false)),
          unchanged("review"),
        ]),
      }),
    ],
  });
}

function fixture(mode = "success") {
  const review = { role: mode === "wrong_role" ? "text_field" : "button", title: "Review report", value: "pending",
    enabled: mode !== "disabled", editable: false, clicks: 0 };
  const note = { role: "text_field", title: "Locked note", value: "", enabled: true, editable: true, clicks: 0 };
  let invocations = 0, writes = 0;
  const application = createRegisteredApplication({
    ...(mode === "missing" ? {} : { review: {
      read: () => review,
      async click(context) {
        invocations++;
        await Promise.resolve();
        context.commit(() => {
          writes++;
          review.value = mode === "wrong_value" ? "still pending" : "done";
          review.clicks += mode === "wrong_count" ? 0 : 1;
          review.enabled = false;
        });
      },
    } }),
    note: { read: () => note, setText(text, context) {
      context.commit(() => { writes++; note.value = text; note.editable = false; note.enabled = false; });
    } },
  });
  const operations = new Map(), requests = [];
  const carrier = createNodeAgentTransport({ executable: defaultWorker, catalog });
  const transport = {
    describe: (signal) => carrier.describe(signal),
    execute(run, signal) {
      return carrier.execute({ ...run, onRequest(request) {
        requests.push(request.operation);
        operations.set(`${request.session}:${request.request_id}`, request.operation);
        if (mode === "stale" && request.operation === "gui.click") application.changed("review");
        run.onRequest(request);
      } }, signal);
    },
    reply(reply) {
      const operation = operations.get(`${reply.session}:${reply.request_id}`);
      // The real registered gateway itself rejects a false acknowledgement.
      // The constructor's source guard remains an additional declared check.
      return carrier.reply(mode === "false_ack" && operation === "gui.click" && reply.status === "ok"
        ? { ...reply, value: false } : reply);
    },
    cancel: (scope) => carrier.cancel(scope),
    close: (endpoint) => carrier.close(endpoint),
  };
  const host = createRegisteredAgentHost({ window: "constructed-board", epoch: randomUUID(), application, transport,
    observations: { before: "objects_before", after: "objects_after" } });
  const session = new AgentSession({ host, provider });
  return { host, session, review, note, requests, counts: () => ({ invocations, writes }),
    close() { session.cancel(); host.close(); } };
}

const parsed = (outcome) => JSON.parse(outcome.report);

test("constructed board tasks discover, complete, self-disable and verify through ordinary Rust sessions", async () => {
  const app = fixture();
  const workflow = new AgentWorkflowSession({ host: app.host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_review", arguments: {} },
      { task: "complete_review", arguments: {}, requires: { fromStep: 0, format: "string", equals: "pending" } },
      { task: "read_review", arguments: {} },
    ]);
  } } });
  try {
    const outcome = await workflow.submit("read, complete and verify review");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.deepEqual(outcome.steps.map((step) => parsed(step).effects), [2, 4, 2]);
    assert.deepEqual(outcome.steps.map((step) => parsed(step).result.value), ["pending", true, "done"]);
    assert.equal(outcome.requirements[0].satisfied, true);
    assert.deepEqual(app.counts(), { invocations: 1, writes: 1 });
    assert.equal(app.review.enabled, false);
    assert.equal(app.note.value, "");
    const repeated = await app.session.submit("task complete_review");
    assert.equal(repeated.status, "failed", "the same shared provider cannot repeat a disabled completion");
    assert.deepEqual(app.counts(), { invocations: 1, writes: 1 });
  } finally { workflow.cancel(); app.close(); }
});

for (const mode of ["missing", "wrong_role", "disabled", "stale"]) {
  test(`constructed click refuses ${mode} controls before the real application mutates`, async () => {
    const app = fixture(mode);
    try {
      const outcome = await app.session.submit("task complete_review");
      assert.equal(outcome.status, "failed");
      assert.deepEqual(app.counts(), { invocations: 0, writes: 0 });
      assert.equal(app.review.value, "pending");
      assert.equal(app.note.value, "");
      if (mode === "stale") assert.ok(app.requests.includes("gui.click"), "the stale revision reaches the actual Bridge fence");
      else assert.ok(!app.requests.includes("gui.click"), "discovery or source guards stop before dispatching a click");
    } finally { app.close(); }
  });
}

for (const mode of ["wrong_value", "wrong_count"]) {
  test(`constructed click does not infer business success from acknowledgement: ${mode}`, async () => {
    const app = fixture(mode);
    try {
      const outcome = await app.session.submit("task complete_review");
      assert.equal(outcome.status, "failed");
      assert.equal(outcome.error, "application_verification_failed");
      assert.equal(outcome.possiblePartialEffects, true);
      assert.equal(parsed(outcome).result.value, true, "the compiled program completed normally");
      assert.equal(parsed(outcome).effects, 4);
      assert.deepEqual(app.counts(), { invocations: 1, writes: 1 }, "the failing report never retries or rolls back the action");
      const read = await app.session.submit("task read_review");
      assert.equal(read.status, "verified", read.error);
      assert.equal(parsed(read).result.value, app.review.value);
    } finally { app.close(); }
  });
}

test("a false acknowledgement from the real gateway cannot hide the already completed action", async () => {
  const app = fixture("false_ack");
  try {
    const outcome = await app.session.submit("task complete_review");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.report, undefined, "the Rust gateway refuses the false acknowledgement before producing a terminal report");
    assert.deepEqual(app.counts(), { invocations: 1, writes: 1 });
    assert.equal(app.review.value, "done");
    assert.equal(app.requests.at(-1), "gui.click", "no post-action observation is dispatched after the rejected acknowledgement");
  } finally { app.close(); }
});

test("constructed text submission permits its own verified readonly and disabled transition", async () => {
  const app = fixture();
  try {
    const text = 'One-time "note" ✓';
    const outcome = await app.session.submit(`task write_locked_note:${text}`);
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(parsed(outcome).effects, 4);
    assert.deepEqual(parsed(outcome).result, { type: "string", value: text });
    assert.equal(app.note.value, text);
    assert.equal(app.note.enabled, false);
    assert.equal(app.note.editable, false);
    assert.equal(app.review.value, "pending");
    assert.deepEqual(app.counts(), { invocations: 0, writes: 1 });
  } finally { app.close(); }
});
