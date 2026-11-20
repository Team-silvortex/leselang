import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { createRegisteredAgentHost, prepareSemanticCatalog } from "../dist/index.js";

const raw = await readFile(new URL("../../../fixtures/host-semantics/tauri-agent-semantics-v1.json", import.meta.url), "utf8");
const plan = prepareSemanticCatalog(raw).prepare("fill_and_save", { text: 'Independent "application" ✓' }).plan;
const signal = () => new AbortController().signal;
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function evidence(text = plan.arguments.text, clicks = 0) {
  const row = (identifier, role, value, count, editable, title) => ({ identifier, role, value,
    clicks: count, editable, title, enabled: true });
  return {
    schema_version: 1, kind: "tauri_task_run", profile: "tauri.typescript",
    result: { type: "boolean", value: true }, effects: 9, fuel_remaining: 9000,
    native_before: [row("save", "button", "old", clicks, false, "Save"),
      row("other", "button", "", 0, false, "Other control"), row("input", "text_field", "old", 0, true, "Text to save")],
    native_after: [row("save", "button", text, clicks + 1, false, "Save"),
      row("other", "button", "", 0, false, "Other control"), row("input", "text_field", text, 0, true, "Text to save")],
  };
}

function fixture(options = {}) {
  const f = { starts: [], replies: [], cancellations: [], closures: [], descriptions: [],
    bridges: [], active: null, visible: evidence().native_before, hooks: {}, writes: 0,
    deferredReplies: [], deferReplies: false, mutation: null, bindingRevision: 0, bindingReads: 0 };
  const row = (id) => f.visible.find((entry) => entry.identifier === id);
  f.application = {
    observe() { f.hooks.observe?.(); return f.visible; },
    attach(bridge) {
      assert.equal(f.active, null);
      f.active = bridge;
      f.bridges.push(bridge);
      for (const id of ["save", "other", "input"]) {
        bridge.register(id, {
          read() { const { identifier: _, ...state } = row(id); return state; },
          async setText(text, context) {
            await (f.mutation?.promise ?? Promise.resolve());
            context.commit(() => { row(id).value = text; f.writes++; });
            f.hooks.afterAction?.(id);
          },
          async click(context) {
            await (f.mutation?.promise ?? Promise.resolve());
            context.commit(() => {
              row(id).clicks++;
              if (id === "save") row(id).value = row("input").value;
              f.writes++;
            });
            f.hooks.afterAction?.(id);
          },
        });
      }
      f.hooks.attach?.(bridge);
    },
    // Deliberately do not close here: the public host owns mutation fencing.
    detach(bridge) {
      if (f.active === bridge) f.active = null;
      f.hooks.detach?.(bridge);
    },
  };
  if (options.bindings) f.application.bindingRevision = () => {
    f.bindingReads++;
    return f.hooks.bindingRevision ? f.hooks.bindingRevision() : f.bindingRevision;
  };
  f.transport = {
    describe(abort) {
      f.descriptions.push(abort);
      return f.hooks.describe?.(abort) ?? Promise.resolve(options.catalog ?? raw);
    },
    execute(run, abort) {
      const pending = deferred();
      f.starts.push({ ...pending, run, signal: abort, serial: 0 });
      return pending.promise;
    },
    reply(reply) {
      f.replies.push(reply);
      f.hooks.reply?.(reply);
      if (f.deferReplies) {
        const pending = deferred(); f.deferredReplies.push(pending); return pending.promise;
      }
    },
    cancel(scope) { f.cancellations.push(scope); return f.hooks.cancel?.(scope); },
    close(endpoint) { f.closures.push(endpoint); return f.hooks.close?.(endpoint); },
  };
  f.host = createRegisteredAgentHost({
    window: "external-main", epoch: "page-1", application: f.application, transport: f.transport,
    observations: options.observations ?? { before: "native_before", after: "native_after" },
    ...(options.executionReports === undefined ? {} : { executionReports: options.executionReports }),
    onSource(source) { f.hooks.source?.(source); },
  });
  f.ready = () => f.host.describe(signal());
  f.request = (start, operation = "gui.list", arguments_ = {}) => ({
    transport_version: 1, ...start.run.scope, request_id: ++start.serial,
    budget_ms: 1000, operation, arguments: arguments_,
  });
  f.call = async (start, operation, arguments_ = {}) => {
    const request = f.request(start, operation, arguments_);
    start.run.onRequest(request);
    for (let turn = 0; turn < 20; turn++) {
      const reply = f.replies.find((item) => item.session === request.session && item.request_id === request.request_id);
      if (reply) return reply;
      await tick();
    }
    assert.fail("the explicit carrier did not receive its correlated reply");
  };
  return f;
}

async function complete(f, report = evidence(), runPlan = plan, abort = signal()) {
  const pending = f.host.execute(runPlan, abort);
  const start = f.starts.at(-1);
  assert.ok(start, "the admitted task reaches the application-owned carrier");
  f.visible = structuredClone(report.native_after ?? report.objects_after);
  const wire = JSON.stringify(report);
  start.resolve(wire);
  assert.equal(await pending, wire);
  return wire;
}

test("public host configuration rejects malformed endpoints, evidence fields and incomplete adapters", () => {
  const valid = () => ({
    window: "external-main", epoch: "fresh-page",
    observations: { before: "native_before", after: "native_after" },
    application: { attach() {}, detach() {}, observe() { return []; } },
    transport: { async describe() { return raw; }, async execute() { return "{}"; }, reply() {}, cancel() {}, close() {} },
  });
  for (const field of ["window", "epoch"]) {
    for (const value of [undefined, null, 42, "", "two words", "path/name", "x".repeat(65), "\ud800"]) {
      const options = valid(); options[field] = value;
      assert.throws(() => createRegisteredAgentHost(options), `${field}: ${String(value)}`);
    }
  }
  for (const observations of [undefined, null, {}, { before: "before" }, { after: "after" },
    { before: "same", after: "same" }, { before: "", after: "after" },
    { before: "nested.before", after: "after" }, { before: "before", after: "after-field" },
    { before: "before", after: "a".repeat(65) }, { before: 42, after: "after" }]) {
    assert.throws(() => createRegisteredAgentHost({ ...valid(), observations }));
  }
  for (const [owner, methods] of [["application", ["attach", "detach", "observe"]],
    ["transport", ["describe", "execute", "reply", "cancel", "close"]]]) {
    for (const method of methods) {
      const options = valid(); delete options[owner][method];
      assert.throws(() => createRegisteredAgentHost(options), `${owner}.${method}`);
    }
    const options = valid(); options[owner] = null;
    assert.throws(() => createRegisteredAgentHost(options), owner);
  }
  for (const bindingRevision of [null, 0, "0", {}, []]) {
    const options = valid(); options.application.bindingRevision = bindingRevision;
    assert.throws(() => createRegisteredAgentHost(options), /invalid_registered_host_options/);
  }
  for (const executionReports of [null, 0, true, "", "v2", {}, []]) {
    assert.throws(() => createRegisteredAgentHost({ ...valid(), executionReports }), /invalid_registered_host_options/);
  }
  assert.throws(() => createRegisteredAgentHost({ ...valid(), onSource: "not a callback" }));
  assert.throws(() => createRegisteredAgentHost(null));
});

test("the public host routes real Promise adapters and verifies captured and fresh observations", async () => {
  const f = fixture(); await f.ready();
  const before = structuredClone(f.visible);
  const pending = f.host.execute(plan, signal());
  const start = f.starts[0];
  assert.equal(start.run.source, plan.source);
  assert.deepEqual(start.run.arguments, plan.arguments);
  assert.deepEqual({ window: start.run.scope.window, epoch: start.run.scope.epoch },
    { window: "external-main", epoch: "page-1" });
  // This is an explicit protocol fixture, not a JavaScript language evaluator.
  assert.equal((await f.call(start, "gui.list")).status, "ok");
  const input = (await f.call(start, "gui.find", { name: "input" })).value;
  const inputBefore = (await f.call(start, "gui.observe", { target: input })).value;
  assert.equal((await f.call(start, "gui.set_text", { target: input,
    expected_revision: inputBefore.revision, text: plan.arguments.text })).status, "ok");
  assert.equal((await f.call(start, "gui.observe", { target: input })).value.value, plan.arguments.text);
  const save = (await f.call(start, "gui.find", { name: "save" })).value;
  const saveBefore = (await f.call(start, "gui.observe", { target: save })).value;
  assert.equal((await f.call(start, "gui.click", { target: save, expected_revision: saveBefore.revision })).status, "ok");
  assert.equal((await f.call(start, "gui.observe", { target: save })).value.clicks, 1);
  const report = { ...evidence(), native_before: before, native_after: structuredClone(f.visible) };
  start.resolve(JSON.stringify(report));
  const wire = await pending;
  assert.equal(f.writes, 2);
  assert.equal(f.active, null);
  assert.equal(f.replies.length, 9);
  f.visible.reverse();
  assert.equal(await f.host.verify(wire, plan, signal()), true, "observation order is not identity");
  assert.equal(await f.host.verify(wire, plan, signal()), false, "completion evidence is single-use");
  const next = f.host.execute(plan, signal());
  assert.notEqual(f.starts[1].run.scope.session, start.run.scope.session);
  f.starts[1].resolve("{}"); await next;
});

test("the trusted host chooses report fields without depending on Tauri names", async () => {
  const renamed = raw.replaceAll('"native_before"', '"objects_before"').replaceAll('"native_after"', '"objects_after"');
  const f = fixture({ catalog: renamed, observations: { before: "objects_before", after: "objects_after" } });
  await f.ready();
  const { native_before, native_after, ...rest } = evidence();
  const report = { ...rest, objects_before: native_before, objects_after: native_after };
  const wire = await complete(f, report);
  assert.equal(await f.host.verify(wire, plan, signal()), true);
});

test("source admission rejects provider changes before attaching controls or executing", async () => {
  const f = fixture(); await f.ready();
  for (const candidate of [
    { ...plan, source: "fn main(text: string) = true" },
    { ...plan, task: "undeclared" },
    { ...plan, arguments: { text: "valid", extra: "not declared" } },
    { summary: plan.summary, source: plan.source, arguments: plan.arguments },
  ]) await assert.rejects(f.host.execute(candidate, signal()));
  assert.equal(f.bridges.length, 0);
  assert.equal(f.starts.length, 0);
});

test("direct execution owns plan data without invoking top-level or argument getters", async () => {
  const f = fixture(); await f.ready();
  let reads = 0;
  for (const field of ["summary", "task", "source", "arguments", "argument_text"]) {
    const candidate = { ...plan, arguments: { ...plan.arguments } };
    const target = field === "argument_text" ? candidate.arguments : candidate;
    const key = field === "argument_text" ? "text" : field;
    const value = target[key];
    Object.defineProperty(target, key, { enumerable: true, get() { reads++; return value; } });
    await assert.rejects(f.host.execute(candidate, signal()), field);
  }
  assert.equal(reads, 0);
  assert.equal(f.starts.length, 0);
  assert.equal(f.bridges.length, 0);
});

test("reports must match both captured initial state and actual current state", async () => {
  for (const fault of ["invented_initial", "changed_current", "different_report", "different_plan", "unrelated_effect", "object_report", "duplicate_json"]) {
    const f = fixture(); await f.ready();
    const report = evidence();
    const pending = f.host.execute(plan, signal());
    if (fault === "invented_initial") report.native_before.find((row) => row.identifier === "input").value = "invented";
    if (fault === "unrelated_effect") report.native_after.find((row) => row.identifier === "other").clicks++;
    f.visible = structuredClone(report.native_after);
    let wire = JSON.stringify(report);
    if (fault === "duplicate_json") wire = wire.replace('"schema_version":1', '"schema_version":1,"schema_version":1');
    f.starts[0].resolve(fault === "object_report" ? report : wire); await pending;
    if (fault === "changed_current") f.visible[0].value = "manual change after execution";
    if (fault === "different_report") wire += " ";
    const candidate = fault === "different_plan" ? { ...plan, arguments: { text: "another value" } } : plan;
    assert.equal(await f.host.verify(fault === "object_report" ? report : wire, candidate, signal()), false, fault);
  }
});

test("invalid observations are refused without invoking accessors or starting transport", async () => {
  for (const fault of ["getter", "duplicate", "extra", "count", "hole"]) {
    const f = fixture(); await f.ready();
    let getterCalls = 0;
    if (fault === "getter") Object.defineProperty(f.visible[0], "value", { enumerable: true,
      get() { getterCalls++; throw new Error("must never read this accessor"); } });
    if (fault === "duplicate") f.visible.push(structuredClone(f.visible[0]));
    if (fault === "extra") f.visible[0].extra = true;
    if (fault === "count") f.visible[0].clicks = 0x1_0000_0000;
    if (fault === "hole") delete f.visible[0];
    await assert.rejects(f.host.execute(plan, signal()));
    assert.equal(getterCalls, 0);
    assert.equal(f.starts.length, 0);
    assert.equal(f.bridges.length, 0);
  }
});

test("description owns the slot until settlement and a closed description cannot publish a catalog", async () => {
  for (const close of [false, true]) {
    const f = fixture(), description = deferred();
    f.hooks.describe = () => description.promise;
    const pending = f.ready();
    await assert.rejects(f.ready());
    await assert.rejects(f.host.execute(plan, signal()));
    assert.equal(f.descriptions.length, 1);
    if (close) f.host.close();
    description.resolve(raw);
    if (close) {
      await assert.rejects(pending);
      await assert.rejects(f.host.execute(plan, signal()));
      assert.equal(f.starts.length, 0);
    } else {
      await pending;
      const execution = f.host.execute(plan, signal());
      await assert.rejects(f.ready());
      f.starts[0].resolve("{}"); await execution;
    }
  }
});

test("aborting a pending description forwards cancellation but keeps the slot until it settles", async () => {
  const f = fixture(), description = deferred();
  f.hooks.describe = () => description.promise;
  const controller = new AbortController();
  const pending = f.host.describe(controller.signal);
  assert.equal(f.descriptions.length, 1);
  assert.equal(f.descriptions[0].aborted, false);
  controller.abort();
  assert.equal(f.descriptions[0].aborted, true, "the carrier receives a live cancellation signal");
  await assert.rejects(f.ready());
  await assert.rejects(f.host.execute(plan, signal()));
  assert.equal(f.descriptions.length, 1, "cancellation does not release an unsettled description");
  description.resolve(raw);
  await assert.rejects(pending);
  await assert.rejects(f.host.execute(plan, signal()), "the late catalog was not installed");
  f.hooks.describe = undefined;
  await f.ready();
  const execution = f.host.execute(plan, signal());
  assert.equal(f.starts.length, 1);
  f.starts[0].resolve("{}"); await execution;

  const untouched = fixture(), preaborted = new AbortController();
  preaborted.abort();
  await assert.rejects(untouched.host.describe(preaborted.signal));
  assert.equal(untouched.descriptions.length, 0);
});

test("abort or close inside observation, attach or source callbacks never starts the carrier", async () => {
  for (const callback of ["observe", "attach", "source"]) {
    for (const mode of ["abort", "close"]) {
      const f = fixture(); await f.ready();
      const controller = new AbortController();
      f.hooks[callback] = () => mode === "abort" ? controller.abort() : f.host.close();
      await assert.rejects(f.host.execute(plan, controller.signal), `${callback}: ${mode}`);
      assert.equal(f.starts.length, 0, `${callback}: ${mode}`);
      assert.equal(f.active, null);
      for (const bridge of f.bridges) {
        assert.equal((await bridge.handle({ transport_version: 1, ...bridge.scope, request_id: 1,
          budget_ms: 1000, operation: "gui.list", arguments: {} })).status, "rejected");
      }
    }
  }
});

test("synchronous callback reentry cannot overlap the captured application operation", async () => {
  for (const callback of ["observe", "attach", "source"]) {
    const f = fixture(); await f.ready();
    let reentered;
    f.hooks[callback] = () => {
      f.hooks[callback] = undefined;
      reentered = assert.rejects(f.host.execute(plan, signal()));
    };
    const pending = f.host.execute(plan, signal());
    await reentered;
    assert.equal(f.starts.length, 1, callback);
    f.starts[0].resolve("{}"); await pending;
  }
});

test("partial attachment failure retires the bridge and never calls the carrier", async () => {
  const f = fixture(); await f.ready();
  f.hooks.attach = () => { throw new Error("partial attachment"); };
  await assert.rejects(f.host.execute(plan, signal()), /partial attachment/);
  assert.equal(f.starts.length, 0);
  assert.equal(f.active, null);
  const bridge = f.bridges[0];
  assert.equal((await bridge.handle({ transport_version: 1, ...bridge.scope, request_id: 1,
    budget_ms: 1000, operation: "gui.list", arguments: {} })).status, "rejected");
  f.hooks.attach = undefined;
  const next = f.host.execute(plan, signal());
  f.starts[0].resolve("{}"); await next;
});

test("cancellation fences pending mutations despite transport errors and stays busy until settlement", async () => {
  for (const mode of ["throw", "reject"]) {
    const f = fixture(); await f.ready();
    f.mutation = deferred();
    f.hooks.cancel = () => {
      if (mode === "throw") throw new Error("cancel transport threw");
      return Promise.reject(new Error("cancel transport rejected"));
    };
    const controller = new AbortController();
    const pending = f.host.execute(plan, controller.signal);
    const start = f.starts[0];
    start.run.onRequest(f.request(start, "gui.click", {
      target: { name: "save", generation: 1 }, expected_revision: 0,
    }));
    await tick();
    controller.abort();
    f.mutation.resolve();
    await tick();
    assert.equal(f.writes, 0);
    assert.equal(f.active, null);
    assert.equal(f.replies[0].status, "uncertain");
    await assert.rejects(f.host.execute(plan, signal()));
    await assert.rejects(f.ready());
    start.resolve(JSON.stringify(evidence()));
    await Promise.allSettled([pending]);
    assert.equal(await f.host.verify(JSON.stringify(evidence()), plan, signal()), false);
    const next = f.host.execute(plan, signal());
    assert.notEqual(f.starts[1].run.scope.session, start.run.scope.session);
    f.starts[1].resolve("{}"); await next;
    assert.equal(f.cancellations.length, 1);
  }
});

test("a late reply failure and requests from the old run cannot retire the next run", async () => {
  const f = fixture(); await f.ready();
  f.deferReplies = true;
  const first = f.host.execute(plan, signal());
  const old = f.starts[0];
  await f.call(old, "gui.list");
  old.resolve("{}"); await first;
  const second = f.host.execute(plan, signal());
  const current = f.starts[1], bridge = f.active;
  f.deferredReplies[0].reject(new Error("late old carrier rejection"));
  await tick();
  assert.equal(f.active, bridge);
  assert.equal(f.cancellations.some((scope) => scope.session === current.run.scope.session), false);
  f.deferReplies = false;
  assert.equal((await f.call(old, "gui.list")).status, "rejected");
  assert.equal((await f.call(current, "gui.list")).status, "ok");
  current.resolve("{}"); await second;
});

test("closing the host is idempotent even when the transport throws or rejects", async () => {
  for (const mode of ["throw", "reject"]) {
    const f = fixture(); await f.ready();
    f.hooks.close = () => {
      if (mode === "throw") throw new Error("close transport threw");
      return Promise.reject(new Error("close transport rejected"));
    };
    f.host.close(); f.host.close();
    await tick();
    assert.deepEqual(f.closures, [{ window: "external-main", epoch: "page-1" }]);
    await assert.rejects(f.ready());
    await assert.rejects(f.host.execute(plan, signal()));
  }
});

test("settled evidence remains bound to its execution signal and callback lifetime", async () => {
  for (const fault of ["late_abort", "verify_close", "verify_abort", "detach_close", "detach_throw"]) {
    const f = fixture(); await f.ready();
    const controller = new AbortController();
    if (fault === "detach_close") f.hooks.detach = () => f.host.close();
    if (fault === "detach_throw") f.hooks.detach = () => { throw new Error("detach failed"); };
    let wire = JSON.stringify(evidence());
    if (fault === "detach_throw") {
      await assert.rejects(complete(f, evidence(), plan, controller.signal), /detach failed/);
    } else {
      wire = await complete(f, evidence(), plan, controller.signal);
    }
    if (fault === "late_abort") controller.abort();
    if (fault === "verify_close") f.hooks.observe = () => f.host.close();
    if (fault === "verify_abort") f.hooks.observe = () => controller.abort();
    assert.equal(await f.host.verify(wire, plan, signal()), false, fault);
    assert.equal(f.active, null);
  }
});

test("teardown cannot consume success evidence before the execution has settled", async () => {
  const f = fixture(); await f.ready();
  let duringDetach;
  const wire = JSON.stringify(evidence());
  f.hooks.detach = () => { duringDetach = f.host.verify(wire, plan, signal()); };
  await complete(f);
  assert.equal(await duringDetach, false);
  assert.equal(await f.host.verify(wire, plan, signal()), true);
});

test("detailed completion identifies semantic and independent observation failures once", async () => {
  const variants = [
    ["identity", "report_identity_mismatch", 1],
    ["budget", "report_budget_mismatch", 1],
    ["predicate", "predicate_failed", 1],
    ["before", "before_observation_mismatch", 2],
    ["after", "after_observation_mismatch", 2],
    ["observation", "observation_invalid", 2],
    ["report", "report_mismatch", 1],
    ["plan", "plan_mismatch", 1],
    ["json", "invalid_report", 1],
    ["object", "invalid_report", 1],
  ];
  for (const [variant, code, expectedReads] of variants) {
    const f = fixture(); await f.ready();
    let reads = 0;
    f.hooks.observe = () => { reads++; };
    const report = evidence();
    const pending = f.host.execute(plan, signal());
    if (variant === "identity") report.profile = "another.application";
    if (variant === "budget") report.effects++;
    if (variant === "predicate") report.native_after.find((row) => row.identifier === "other").clicks++;
    if (variant === "before") report.native_before.find((row) => row.identifier === "input").value = "invented";
    f.visible = structuredClone(report.native_after);
    let wire = JSON.stringify(report);
    if (variant === "json") wire = wire.replace('"schema_version":1', '"schema_version":1,"schema_version":1');
    if (variant === "object") wire = report;
    f.starts[0].resolve(wire);
    await pending;
    if (variant === "after") f.visible[0].value = "manual change";
    if (variant === "observation") f.hooks.observe = () => { reads++; throw new Error("private application data"); };
    if (variant === "report") wire += " ";
    const proposed = variant === "plan" ? { ...plan, arguments: { text: "other input" } } : plan;
    const result = await f.host.verifyDetailed(wire, proposed, signal());
    assert.equal(result.verified, false, variant);
    assert.equal(result.diagnostic.code, code, variant);
    assert.ok(Object.isFrozen(result) && Object.isFrozen(result.diagnostic));
    if (variant === "predicate") assert.equal(result.diagnostic.predicate, "other_unchanged");
    assert.equal(reads, expectedReads, variant);
    assert.deepEqual(await f.host.verifyDetailed(wire, proposed, signal()),
      { verified: false, diagnostic: { code: "completion_unavailable" } });
    assert.equal(reads, expectedReads, "repeated verification performs no application read");
    assert.equal(f.host.verify(wire, proposed, signal()), false);
  }
});

test("detailed verification shares Boolean consumption and preserves callback lifetime fences", async () => {
  for (const mode of ["detailed_first", "boolean_first", "close", "abort", "reentrant", "detach_reentry"]) {
    const f = fixture(); await f.ready();
    const controller = new AbortController();
    let nested;
    if (mode === "detach_reentry") {
      f.hooks.detach = () => { nested = f.host.verifyDetailed(JSON.stringify(evidence()), plan, signal()); };
    }
    const wire = await complete(f, evidence(), plan, controller.signal);
    if (mode === "close") f.hooks.observe = () => f.host.close();
    if (mode === "abort") f.hooks.observe = () => controller.abort();
    if (mode === "reentrant") f.hooks.observe = () => { nested = f.host.verifyDetailed(wire, plan, signal()); };
    if (mode === "boolean_first") assert.equal(f.host.verify(wire, plan, signal()), true);
    const result = await f.host.verifyDetailed(wire, plan, signal());
    if (mode === "close" || mode === "abort") {
      assert.deepEqual(result, { verified: false, diagnostic: { code: "verification_interrupted" } });
    } else if (mode === "boolean_first") {
      assert.deepEqual(result, { verified: false, diagnostic: { code: "completion_unavailable" } });
    } else assert.deepEqual(result, { verified: true });
    if (mode === "reentrant" || mode === "detach_reentry") {
      assert.deepEqual(nested, { verified: false, diagnostic: { code: "completion_unavailable" } });
    }
    assert.equal(f.host.verify(wire, plan, signal()), false);
    assert.equal(f.active, null);
  }
});

test("optional binding revisions are captured once and must be nonnegative safe integers", async () => {
  for (const invalid of [undefined, null, false, "0", -1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    const f = fixture({ bindings: true }); await f.ready();
    f.hooks.bindingRevision = () => invalid;
    await assert.rejects(f.host.execute(plan, signal()), /invalid_registered_binding_revision/);
    assert.equal(f.starts.length, 0);
    assert.equal(f.bridges.length, 0);
  }
  const f = fixture({ bindings: true }); await f.ready();
  f.application.bindingRevision = () => { throw new Error("replacement method must not run"); };
  const wire = await complete(f);
  assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()), { verified: true });
  assert.ok(f.bindingReads > 0, "the originally captured method is used");
});

test("binding changes in initial observations, attachment or source callbacks never start transport", async () => {
  for (const callback of ["observe", "attach", "source"]) {
    const f = fixture({ bindings: true }); await f.ready();
    f.hooks[callback] = () => { f.bindingRevision++; };
    await assert.rejects(f.host.execute(plan, signal()), /application_bindings_changed/);
    assert.equal(f.starts.length, 0, callback);
    assert.equal(f.writes, 0, callback);
    assert.equal(f.active, null, callback);
    assert.ok(f.bridges.every((bridge) => bridge.closed));
  }
});

test("binding changes stop request dispatch and suppress a completed stale reply", async () => {
  for (const phase of ["before_request", "after_action"]) {
    const f = fixture({ bindings: true }); await f.ready();
    f.hooks.cancel = (scope) => f.starts.find((start) => start.run.scope.session === scope.session)
      ?.reject(new Error("transport cancelled"));
    const execution = f.host.execute(plan, signal());
    const failed = assert.rejects(execution, /application_bindings_changed/);
    const start = f.starts[0];
    if (phase === "before_request") f.bindingRevision++;
    else f.hooks.afterAction = () => { f.bindingRevision++; };
    start.run.onRequest(f.request(start, "gui.click", { target: { name: "save", generation: 1 }, expected_revision: 0 }));
    await failed; await tick();
    assert.equal(f.writes, phase === "before_request" ? 0 : 1);
    assert.equal(f.cancellations.length, 1);
    assert.equal(f.bridges[0].closed, true);
    assert.equal(f.active, null);
    assert.equal(f.replies.filter((reply) => reply.status === "ok").length, 0, "no stale success reaches the carrier");
    assert.equal(f.host.verifyDetailed("{}", plan, signal()).diagnostic.code, "completion_unavailable");
  }
});

test("same-value remounts, ABA and teardown changes invalidate exactly one completed report", async () => {
  for (const phase of ["after_return", "aba", "detach", "verify_observe", "verify_observe_throw", "invalid_revision"]) {
    const f = fixture({ bindings: true }); await f.ready();
    if (phase === "detach") f.hooks.detach = () => { f.bindingRevision++; };
    const wire = await complete(f);
    let reads = 0;
    f.hooks.observe = () => {
      reads++;
      if (phase.startsWith("verify_observe")) f.bindingRevision++;
      if (phase === "verify_observe_throw") throw new Error("reader was replaced");
    };
    if (phase === "after_return") f.bindingRevision++;
    if (phase === "aba") f.bindingRevision += 2;
    if (phase === "invalid_revision") f.hooks.bindingRevision = () => NaN;
    assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()),
      { verified: false, diagnostic: { code: "application_bindings_changed" } }, phase);
    assert.equal(reads, phase.startsWith("verify_observe") ? 1 : 0);
    assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()),
      { verified: false, diagnostic: { code: "completion_unavailable" } }, phase);
  }
});

test("binding diagnostics remain correlated and cancellation still has precedence", async () => {
  for (const phase of ["plan", "report", "abort", "close_in_revision"]) {
    const f = fixture({ bindings: true }); await f.ready();
    const controller = new AbortController();
    const wire = await complete(f, evidence(), plan, controller.signal);
    f.bindingRevision++;
    if (phase === "abort") controller.abort();
    if (phase === "close_in_revision") f.hooks.bindingRevision = () => { f.host.close(); return f.bindingRevision; };
    const result = f.host.verifyDetailed(phase === "report" ? wire + " " : wire,
      phase === "plan" ? { ...plan, arguments: { text: "changed" } } : plan, signal());
    assert.equal(result.diagnostic.code, phase === "plan" ? "plan_mismatch"
      : phase === "report" ? "report_mismatch" : "verification_interrupted", phase);
  }
});

test("late requests from a retired binding attempt cannot inspect or retire a replacement session", async () => {
  const f = fixture({ bindings: true }); await f.ready();
  f.hooks.cancel = (scope) => f.starts.find((start) => start.run.scope.session === scope.session)
    ?.reject(new Error("transport cancelled"));
  const execution = f.host.execute(plan, signal());
  const failed = assert.rejects(execution, /application_bindings_changed/);
  const old = f.starts[0];
  f.bindingRevision++;
  old.run.onRequest(f.request(old));
  await failed; await tick();
  await f.ready();
  const replacement = f.host.execute(plan, signal());
  const start = f.starts[1];
  const reads = f.bindingReads;
  old.run.onRequest(f.request(old, "gui.click", { target: { name: "save", generation: 1 }, expected_revision: 0 }));
  await tick();
  assert.equal(f.bindingReads, reads, "old delivery does not call current application revision hooks");
  assert.equal(f.cancellations.length, 1);
  assert.equal(f.bridges[1].closed, false);
  assert.equal((await f.call(start, "gui.list")).status, "ok");
  f.visible = evidence().native_after;
  const wire = JSON.stringify(evidence()); start.resolve(wire);
  assert.equal(await replacement, wire);
  assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()), { verified: true });
});

function failedExecution(code = "execution_object_unavailable") {
  return { schema_version: 1, profile: "tauri.typescript", kind: "tauri_task_run",
    status: code === "execution_cancelled" ? "cancelled" : "failed",
    error: "private host error", possible_partial_effects: true, execution_failure: { code } };
}

test("registered hosts expose only an explicitly selected immutable report policy captured once", async () => {
  const legacy = fixture();
  assert.equal(Object.hasOwn(legacy.host, "executionReports"), false);
  const f = fixture();
  let reads = 0, selected = "v1";
  f.host = createRegisteredAgentHost({ window: "configured", epoch: "fresh",
    application: f.application, transport: f.transport,
    observations: { before: "native_before", after: "native_after" },
    get executionReports() { reads++; return selected; },
  });
  assert.equal(reads, 1);
  selected = undefined;
  assert.equal(f.host.executionReports, "v1");
  assert.throws(() => { f.host.executionReports = undefined; }, TypeError);
  assert.throws(() => { delete f.host.executionReports; }, TypeError);
  await f.ready();
  const pending = f.host.execute(plan, signal()), wire = JSON.stringify(failedExecution());
  f.starts[0].resolve(wire);
  assert.equal(await pending, wire);
  assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()),
    { verified: false, diagnostic: { code: "execution_object_unavailable" } });
  assert.equal(reads, 1, "description, execution and verification use the captured policy");
});

test("explicit execution failures preserve exact completion evidence and prior application mutations", async () => {
  const f = fixture({ executionReports: "v1" }); await f.ready();
  const pending = f.host.execute(plan, signal()), start = f.starts[0];
  const input = (await f.call(start, "gui.find", { name: "input" })).value;
  const observed = (await f.call(start, "gui.observe", { target: input })).value;
  assert.equal((await f.call(start, "gui.set_text", { target: input, expected_revision: observed.revision,
    text: plan.arguments.text })).status, "ok");
  const wire = JSON.stringify(failedExecution());
  start.resolve(wire);
  assert.equal(await pending, wire, "transport evidence is preserved byte-for-byte");
  let observedAgain = 0;
  f.hooks.observe = () => { observedAgain++; };
  assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()),
    { verified: false, diagnostic: { code: "execution_object_unavailable" } });
  assert.equal(observedAgain, 0, "a terminal failed report does not seek missing success snapshots");
  assert.equal(f.writes, 1);
  assert.equal(f.visible.find((row) => row.identifier === "input").value, plan.arguments.text);
  assert.equal(f.visible.find((row) => row.identifier === "save").clicks, 0);
  assert.deepEqual(f.host.verifyDetailed(wire, plan, signal()),
    { verified: false, diagnostic: { code: "completion_unavailable" } });
  assert.equal(f.cancellations.length, 0, "diagnosis does not retry or manufacture cancellation");
});

test("failure reasons remain subordinate to exact completion identity, binding lifetime and cancellation", async () => {
  for (const mode of ["plan", "report", "bindings", "abort", "closed", "foreign_identity"]) {
    const f = fixture({ executionReports: "v1", bindings: true }); await f.ready();
    const controller = new AbortController();
    const pending = f.host.execute(plan, controller.signal);
    const failure = failedExecution();
    if (mode === "foreign_identity") failure.profile = "other.app";
    const wire = JSON.stringify(failure); f.starts[0].resolve(wire); await pending;
    if (mode === "bindings") f.bindingRevision++;
    if (mode === "abort") controller.abort();
    if (mode === "closed") f.host.close();
    const result = f.host.verifyDetailed(mode === "report" ? wire + " " : wire,
      mode === "plan" ? { ...plan, arguments: { text: "different" } } : plan, signal());
    assert.deepEqual(result, { verified: false, diagnostic: { code: {
      plan: "plan_mismatch", report: "report_mismatch", bindings: "application_bindings_changed",
      abort: "verification_interrupted", closed: "verification_interrupted", foreign_identity: "report_identity_mismatch",
    }[mode] } }, mode);
  }
});

test("unconfigured registered hosts preserve legacy status fields and minimal failure diagnostics", async () => {
  const legacy = fixture(); await legacy.ready();
  const successWithMetadata = { ...evidence(), status: "application-defined",
    execution_failure: { code: "business-state", extra: true } };
  const success = await complete(legacy, successWithMetadata);
  assert.equal(legacy.host.verify(success, plan, signal()), true);
  const pending = legacy.host.execute(plan, signal()), failure = JSON.stringify(failedExecution());
  legacy.starts.at(-1).resolve(failure); await pending;
  assert.deepEqual(legacy.host.verifyDetailed(failure, plan, signal()),
    { verified: false, diagnostic: { code: "report_budget_mismatch" } });
});
