import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { Bridge } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";

const faults = new Set(["none", "wrong_publish_target", "unversioned_edit", "reject_click",
  "unacknowledged_click", "acknowledge_without_edit", "final_read_invalid", "publication_superseded",
  "disabled_draft", "readonly_draft", "cancel_before_edit", "cancel_after_edit",
  "replaced_draft_at_write", "external_edit_at_write", "write_grant_revoked"]);

// The application outlives the worker and Bridge used for each individual task.
function createArticle(fault) {
  const state = { draft: "", published: "", edits: 0, clicks: 0 };
  function draftAdapter() {
    return {
      read: () => ({ title: "Draft", role: "text_field", value: state.draft, clicks: 0,
        enabled: fault !== "disabled_draft", editable: fault !== "readonly_draft" }),
      setText(text, context) {
        if (fault !== "acknowledge_without_edit") {
          context.commit(() => { state.draft = text; state.edits++; });
        }
      },
    };
  }
  function attach(bridge) {
    bridge.register("draft", draftAdapter());
    bridge.register("publish", {
      read: () => ({ title: "Publish", role: "button", value: state.published, clicks: state.clicks,
        enabled: true, editable: false }),
      click(context) {
        context.commit(() => { state.published = state.draft; state.clicks++; });
      },
    });
  }
  return { state, attach, replaceDraft(bridge) { bridge.replace("draft", draftAdapter()); } };
}

// Perturb only the selected exchange on the primary session. Each probe gets
// a new adapter binding over the same model and has no injected wire faults.
function articleExchange(bridge, app, fault, supersedingText) {
  const { state } = app;
  let draftTarget, draftRevision;
  return async (request) => {
    const operation = request.operation.slice("gui.".length);
    const target = request.arguments.name ?? request.arguments.target?.name;
    if (operation === "click" && fault === "reject_click") {
      return { ...bridge.scope, transport_version: 1, request_id: request.request_id,
        status: "rejected", value: null, error: "cannot_publish" };
    }

    let rejection;
    if (operation === "set_text") {
      switch (fault) {
        case "replaced_draft_at_write":
          app.replaceDraft(bridge);
          rejection = "stale_object";
          break;
        case "external_edit_at_write":
          state.draft = supersedingText;
          bridge.changed("draft");
          rejection = "stale_revision";
          break;
        case "write_grant_revoked":
          bridge.revoke();
          rejection = "revoked";
          break;
      }
    }
    const reply = await bridge.handle(request);
    if (rejection) {
      assert.deepEqual([reply.status, reply.error], ["rejected", rejection]);
      return reply;
    }
    assert.equal(reply.status, "ok", reply.error ?? "unexpected bridge failure");
    if (operation === "find") {
      if (target === "draft") draftTarget = reply.value;
      if (target === "publish" && state.edits > 0 && fault === "wrong_publish_target") {
        return { ...reply, value: draftTarget };
      }
    }
    if (operation === "observe" && target === "draft") {
      if (state.edits === 0) draftRevision = reply.value.revision;
      if (state.edits > 0 && fault === "unversioned_edit") {
        return { ...reply, value: { ...reply.value, revision: draftRevision } };
      }
      if (state.clicks > 0 && fault === "final_read_invalid") {
        return { ...reply, value: { ...reply.value, identifier: "foreign" } };
      }
      if (state.clicks > 0 && fault === "publication_superseded") {
        state.published = supersedingText;
        bridge.changed("publish");
      }
    }
    if (operation === "click" && fault === "unacknowledged_click") return { ...reply, value: false };
    return reply;
  };
}

async function runWorker(executable, app, source, args, { fault = "none", supersedingText, readOnly = false, requestTimes } = {}) {
  const scope = { window: "article", epoch: randomUUID(), session: randomUUID() };
  const bridge = new Bridge(scope);
  const transport = createNodeAgentTransport({ executable, catalog: (base) => base, executionReports: "v1" });
  const controller = new AbortController();
  const trace = [];
  const exchange = articleExchange(bridge, app, fault, supersedingText);
  let callbackError;
  let cancelledAtWrite = false;
  function cancelEdit() {
    cancelledAtWrite = true;
    bridge.cancel(scope.session);
    controller.abort();
  }
  async function deliver(request) {
    const operation = request.operation.slice("gui.".length);
    const target = request.arguments.name ?? request.arguments.target?.name;
    trace.push(target === undefined ? operation : `${operation}:${target}`);
    if (readOnly) assert.ok(["list", "find", "observe"].includes(operation), "write requested during readback");
    if (operation === "set_text" && fault === "cancel_before_edit") {
      cancelEdit();
      return;
    }
    const reply = await exchange(request);
    if (operation === "set_text" && fault === "cancel_after_edit") {
      assert.equal(app.state.edits, 1);
      assert.equal(app.state.draft, request.arguments.text);
      cancelEdit();
      return;
    }
    await transport.reply(reply);
  }
  try {
    app.attach(bridge);
    const raw = await transport.execute({ scope, source, arguments: args, onRequest(request) {
      if (requestTimes) {
        const now = process.hrtime.bigint();
        requestTimes.first ??= now;
        requestTimes.last = now;
      }
      deliver(request).catch((error) => {
        callbackError = error;
        controller.abort();
      });
    } }, controller.signal);
    const report = JSON.parse(raw);
    assert.equal(report?.kind, "javascript_task_run");
    assert.equal(report.profile, "javascript.registered");
    return { report, trace };
  } catch (error) {
    if (callbackError) throw callbackError;
    if (cancelledAtWrite && error?.message === "node_worker_cancelled") return { report: null, trace };
    throw error;
  } finally {
    bridge.close();
    transport.close(scope);
  }
}

async function readValue(executable, app, source) {
  const { report } = await runWorker(executable, app, source, {}, { readOnly: true });
  assert.equal(report.result.type, "string");
  assert.equal(report.effects, 2);
  return report.result.value;
}

function summarize(report, app, text) {
  if (report === null) return { outcome: "cancelled", result: null, effects: null, verified: false, report_after: null };
  if (report.status === "failed") {
    assert.equal(report.execution_failure.code, "execution_failed");
    assert.equal(report.possible_partial_effects, true);
    for (const field of ["effects", "result", "objects_after"]) assert.ok(!Object.hasOwn(report, field));
    return { outcome: report.execution_failure.code, result: null, effects: null, verified: false, report_after: null };
  }
  assert.equal(report.status, undefined, "unexpected terminal status");
  assert.equal(report.result.type, "boolean");
  const { value: draft } = report.objects_after.find((control) => control.identifier === "draft");
  const { value: published, clicks } = report.objects_after.find((control) => control.identifier === "publish");
  const verified = report.result.value === true && draft === text && published === text && clicks === 1
    && app.state.draft === text && app.state.published === text && app.state.edits === 1 && app.state.clicks === 1;
  return { outcome: "reported", result: report.result.value, effects: report.effects, verified,
    report_after: { draft, published, clicks } };
}

export async function checkRegisteredGui(executable, fixturePath, onCase, onTiming) {
  const clock = onTiming ? process.hrtime.bigint : undefined;
  const fixture = JSON.parse(await readFile(fixturePath, "utf8"));
  assert.equal(fixture.version, 1);
  assert.ok(fixture.cases.length > 0);
  assert.equal(new Set(fixture.cases.map(({ id }) => id)).size, fixture.cases.length, "duplicate GUI case");
  const [program, readDraft, readPublication] = await Promise.all(
    [fixture.program, fixture.read_draft, fixture.read_publication].map(
      (name) => readFile(resolve(dirname(fixturePath), name), "utf8")));
  for (const { id, fault, expected } of fixture.cases) {
    assert.ok(faults.has(fault), `unsupported GUI fault ${fault}`);
    const app = createArticle(fault);
    const requestTimes = onTiming ? {} : undefined;
    const primaryStart = clock?.();
    const { report, trace } = await runWorker(executable, app, program, fixture.arguments,
      { fault, supersedingText: fixture.superseding_text, requestTimes });
    const primaryEnd = clock?.();
    const result = summarize(report, app, fixture.arguments.text);
    const commits = { edits: app.state.edits, clicks: app.state.clicks };
    const readbackStart = clock?.();
    const draft = await readValue(executable, app, readDraft);
    const published = await readValue(executable, app, readPublication);
    const readbackEnd = clock?.();
    assert.deepEqual(app.state, { draft, published, ...commits }, id);
    const observed = { ...result, readback: { draft, published }, commits, trace };
    assert.deepEqual(observed, expected, id);
    onCase({ id, observed });
    if (onTiming) {
      onTiming({ id, primary_ns: Number(primaryEnd - primaryStart),
        request_span_ns: requestTimes.first === undefined ? 0 : Number(requestTimes.last - requestTimes.first),
        readback_ns: Number(readbackEnd - readbackStart) });
    }
  }
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  const args = process.argv.slice(2);
  const benchmark = args[0] === "--benchmark-registered-gui";
  if (benchmark) args.shift();
  assert.equal(args.length, 2, "usage: node registered-gui.mjs [--benchmark-registered-gui] WORKER FIXTURE");
  await checkRegisteredGui(resolve(args[0]), resolve(args[1]),
    (record) => console.log(`LESE_GUI ${JSON.stringify(record)}`),
    benchmark ? (record) => console.log(`LESE_GUI_TIMING ${JSON.stringify(record)}`) : undefined);
}
