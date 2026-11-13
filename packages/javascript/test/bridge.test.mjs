import assert from "node:assert/strict";
import test from "node:test";
import { Bridge } from "@leselang/bridge";

const scope = { window: "main", epoch: "document-1", session: "session-1" };
const scalar = (role, extra = {}) => ({ role, title: "Title", value: "", enabled: true, editable: role === "text_field", clicks: 0, ...extra });
const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const turn = () => new Promise((resolve) => setImmediate(resolve));

function fixture() {
  const bridge = new Bridge(scope);
  const input = scalar("text_field");
  const save = scalar("button");
  const other = scalar("button");
  const counts = { text: 0, click: 0 };
  const inputAdapter = {
    read: () => input,
    setText: (text, context) => context.commit(() => {
      counts.text += 1;
      input.value = text;
      bridge.changed("input");
      bridge.changed("save");
    }),
  };
  bridge.register("input", inputAdapter);
  bridge.register("save", {
    read: () => save,
    click: async (context) => {
      await Promise.resolve();
      context.commit(() => {
        counts.click += 1;
        save.value = input.value;
        save.clicks += 1;
        save.title = "ready";
        bridge.changed("save");
      });
    },
  });
  bridge.register("other", { read: () => other });
  let serial = 0;
  const request = (operation, args = {}, extra = {}) => ({ transport_version: 1, ...scope,
    request_id: ++serial, budget_ms: 5000, operation, arguments: args, ...extra });
  const send = (operation, args = {}, extra = {}) => bridge.handle(request(operation, args, extra));
  const find = async (name) => {
    const reply = await send("gui.find", { name });
    assert.equal(reply.status, "ok");
    return reply.value;
  };
  const observe = async (target) => {
    const reply = await send("gui.observe", { target });
    assert.equal(reply.status, "ok");
    return reply.value;
  };
  return { bridge, input, save, other, counts, inputAdapter, request, send, find, observe };
}

test("real adapter callbacks use nine requests and fresh postcondition snapshots", async () => {
  const f = fixture();
  const ids = await f.send("gui.list");
  assert.deepEqual(ids.value, ["input", "save", "other"]);
  const target = await f.find("input");
  const before = await f.observe(target);
  const text = "保存 \"✓\" \\\n";
  const set = await f.send("gui.set_text", { target, expected_revision: before.revision, text });
  assert.equal(set.status, "ok");
  assert.equal(set.value, true);
  const after = await f.observe(target);
  assert.equal(after.value, text);
  assert.ok(after.revision > before.revision);
  assert.equal(before.value, "", "historical observation remains unchanged");
  const button = await f.find("save");
  const beforeSave = await f.observe(button);
  assert.equal((await f.send("gui.click", { target: button, expected_revision: beforeSave.revision })).status, "ok");
  const afterSave = await f.observe(button);
  assert.equal(afterSave.clicks, beforeSave.clicks + 1);
  assert.equal(afterSave.value, text);
  assert.equal(f.counts.text, 1);
  assert.equal(f.counts.click, 1);
  assert.deepEqual(f.other, scalar("button"));
  assert.equal(ids.request_id, 1);
  assert.equal(after.identifier, "input");
  assert.equal(after.generation, target.generation);
  assert.deepEqual(Object.keys(after).sort(), ["clicks", "editable", "enabled", "generation", "identifier", "revision", "role", "title", "value"]);
});

test("malformed envelopes, unsupported operations and scalar bounds never call adapters", async () => {
  for (const change of [
    (request) => { request.transport_version = 2; },
    (request) => { request.budget_ms = 0; },
    (request) => { request.budget_ms = 5001; },
    (request) => { request.request_id = -1; },
    (request) => { request.request_id = 2 ** 32; },
    (request) => { request.operation = "eval"; },
    (request) => { request.unexpected = true; },
    (request) => { request.arguments.extra = 1; },
    (request) => { request.arguments.text = "x".repeat(4097); },
    (request) => { request.arguments.text = "\ud800"; },
    (request) => { request.arguments.text = "\udc00"; },
    (request) => { request.arguments.target.generation = 0; },
    (request) => { request.arguments.expected_revision = Number.NaN; },
  ]) {
    const f = fixture();
    const request = f.request("gui.set_text", { target: { name: "input", generation: 1 }, expected_revision: 0, text: "x" });
    change(request);
    assert.equal((await f.bridge.handle(request)).status, "rejected");
    assert.equal(f.counts.text, 0);
  }
  for (const input of [null, [], 1, "{}", {}, { request_id: 1 }]) {
    assert.equal((await fixture().bridge.handle(input)).status, "rejected");
  }
});

test("window, document and session bindings are exact and invalid messages do not consume serials", async () => {
  for (const field of ["window", "epoch", "session"]) {
    const f = fixture();
    const wrong = f.request("gui.list", {}, { [field]: "other" });
    const rejected = await f.bridge.handle(wrong);
    assert.equal(rejected.status, "rejected");
    assert.equal(rejected.request_id, 1);
    assert.equal(rejected.value, null);
    const good = await f.bridge.handle({ ...wrong, [field]: scope[field] });
    assert.equal(good.status, "ok");
  }
});

test("duplicate and skipped serials cannot replay an action", async () => {
  const f = fixture();
  const request = f.request("gui.set_text", { target: { name: "input", generation: 1 }, expected_revision: 0, text: "once" });
  assert.equal((await f.bridge.handle(request)).status, "ok");
  assert.equal((await f.bridge.handle(request)).error, "request_order");
  assert.equal((await f.bridge.handle({ ...request, request_id: 3 })).error, "request_order");
  assert.equal(f.counts.text, 1);
});

test("admission owns request data and cancellation before callback entry performs no work", async () => {
  const f = fixture();
  const request = f.request("gui.set_text", { target: { name: "input", generation: 1 }, expected_revision: 0, text: "admitted" });
  const pending = f.bridge.handle(request);
  request.arguments.text = "x".repeat(4097);
  request.arguments.target.name = "other";
  request.request_id = 42;
  assert.equal((await pending).status, "ok");
  assert.equal(f.input.value, "admitted");
  const cancelled = fixture();
  const blocked = cancelled.send("gui.set_text", { target: { name: "input", generation: 1 }, expected_revision: 0, text: "no" });
  cancelled.bridge.cancel(scope.session);
  assert.equal((await blocked).status, "rejected");
  assert.equal(cancelled.counts.text, 0);
});

test("replacement and unregister/re-register invalidate same-name old targets", async () => {
  for (const replace of [true, false]) {
    const f = fixture();
    const old = await f.find("input");
    if (replace) f.bridge.replace("input", f.inputAdapter);
    else { f.bridge.unregister("input"); f.bridge.register("input", f.inputAdapter); }
    assert.equal((await f.send("gui.observe", { target: old })).error, "stale_object");
    const current = await f.find("input");
    assert.ok(current.generation > old.generation);
  }
});

test("explicit ABA and external observed changes both fence old revisions", async () => {
  for (const aba of [true, false]) {
    const f = fixture();
    const target = await f.find("input");
    const before = await f.observe(target);
    if (aba) {
      f.input.value = "temporary";
      f.bridge.changed("input");
      f.input.value = "";
      f.bridge.changed("input");
    } else f.input.value = "external";
    assert.equal((await f.send("gui.set_text", { target, expected_revision: before.revision, text: "must not run" })).error, "stale_revision");
    assert.equal(f.counts.text, 0);
  }
});

test("roles, disabled state and editability are checked before action invocation", async () => {
  for (const kind of ["disabled", "readonly", "role"]) {
    const f = fixture();
    if (kind === "disabled") f.input.enabled = false;
    if (kind === "readonly") f.input.editable = false;
    if (kind === "role") f.input.role = "button";
    const target = await f.find("input");
    const current = await f.observe(target);
    const reply = await f.send("gui.set_text", { target, expected_revision: current.revision, text: "x" });
    assert.equal(reply.status, "rejected");
    assert.equal(f.counts.text, 0);
  }
});

test("async callback must cross the commit gate after every await", async () => {
  const f = fixture();
  const wait = deferred();
  let commits = 0;
  f.bridge.replace("save", { read: () => f.save, click: async (context) => {
    await wait.promise;
    context.commit(() => { commits += 1; f.save.clicks += 1; });
  }});
  const target = await f.find("save");
  const before = await f.observe(target);
  const pending = f.send("gui.click", { target, expected_revision: before.revision });
  await turn();
  f.bridge.changed("save");
  wait.resolve();
  const reply = await pending;
  assert.equal(reply.status, "uncertain");
  assert.equal(reply.error, "stale_revision");
  assert.equal(commits, 0);
});

test("one pending Promise rejects concurrent dispatch and replacement prevents its late mutation", async () => {
  const f = fixture();
  const wait = deferred();
  let writes = 0;
  f.bridge.replace("save", { read: () => f.save, click: async (context) => {
    await wait.promise;
    context.commit(() => { writes += 1; });
  }});
  const target = await f.find("save");
  const before = await f.observe(target);
  const pending = f.send("gui.click", { target, expected_revision: before.revision });
  await turn();
  assert.equal((await f.send("gui.list")).error, "busy");
  f.bridge.replace("save", { read: () => f.save });
  wait.resolve();
  assert.equal((await pending).error, "stale_object");
  assert.equal(writes, 0);
});

test("cancel, close and revoke refuse delayed commits and later requests", async () => {
  for (const stop of ["cancel", "close", "revoke"]) {
    const f = fixture();
    const wait = deferred();
    let writes = 0;
    f.bridge.replace("save", { read: () => f.save, click: async (context) => {
      await wait.promise;
      context.commit(() => { writes += 1; });
    }});
    const target = await f.find("save");
    const before = await f.observe(target);
    const pending = f.send("gui.click", { target, expected_revision: before.revision });
    await turn();
    f.bridge[stop](scope.session);
    assert.equal((await pending).status, "uncertain");
    wait.resolve();
    await turn();
    assert.equal(writes, 0);
    assert.equal((await f.send("gui.list")).status, "rejected");
  }
});

test("cancel after commit and rejected Promise preserve already performed effects", async () => {
  for (const cancel of [true, false]) {
    const f = fixture();
    const wait = deferred();
    f.bridge.replace("save", { read: () => f.save, click: async (context) => {
      context.commit(() => { f.save.clicks += 1; f.save.value = "already saved"; });
      await wait.promise;
    }});
    const target = await f.find("save");
    const before = await f.observe(target);
    const pending = f.send("gui.click", { target, expected_revision: before.revision });
    await turn();
    if (cancel) { f.bridge.cancel(scope.session); wait.resolve(); }
    else wait.reject(new Error("private implementation detail"));
    const reply = await pending;
    assert.equal(reply.status, "uncertain");
    assert.equal(reply.value, null);
    assert.ok(!JSON.stringify(reply).includes("private implementation"));
    assert.equal(f.save.clicks, 1);
    assert.equal(f.save.value, "already saved");
  }
});

test("a deadline settles the pending request and refuses a later Promise completion", async () => {
  const f = fixture();
  const wait = deferred();
  let commits = 0;
  f.bridge.replace("save", { read: () => f.save, click: async (context) => {
    await wait.promise;
    context.commit(() => { commits += 1; });
  }});
  const target = await f.find("save");
  const before = await f.observe(target);
  const reply = await f.send("gui.click", { target, expected_revision: before.revision }, { budget_ms: 20 });
  assert.equal(reply.status, "uncertain");
  assert.equal(reply.error, "deadline");
  wait.resolve();
  await turn();
  assert.equal(commits, 0);
});

test("an early timer callback keeps its deadline cause instead of becoming cancellation", async () => {
  const f = fixture();
  const wait = deferred();
  let commits = 0;
  f.bridge.replace("save", { read: () => f.save, click: async (context) => {
    await wait.promise;
    context.commit(() => { commits += 1; });
  }});
  const target = await f.find("save");
  const before = await f.observe(target);
  const original = globalThis.setTimeout;
  try {
    // Deterministically model timer rounding before the fractional clock
    // deadline. Only the timer cause changes; the same abort fence still runs.
    globalThis.setTimeout = (callback) => original(callback, 0);
    const reply = await f.send("gui.click", { target, expected_revision: before.revision }, { budget_ms: 5000 });
    assert.equal(reply.status, "uncertain");
    assert.equal(reply.error, "deadline");
  } finally {
    globalThis.setTimeout = original;
    wait.resolve();
  }
  await turn();
  assert.equal(commits, 0);
});

test("object inventory and field byte bounds are checked without truncation", async () => {
  const bridge = new Bridge(scope);
  for (let index = 0; index < 64; index += 1) bridge.register(`control${index}`, { read: () => scalar("button") });
  assert.throws(() => bridge.register("overflow", { read: () => scalar("button") }), /object_limit/);
  assert.throws(() => bridge.register("control0", { read: () => scalar("button") }), /duplicate_object/);
  const f = fixture();
  f.input.value = "x".repeat(4097);
  assert.equal((await f.send("gui.find", { name: "input" })).error, "invalid_state");
  f.input.value = "x".repeat(4096);
  assert.equal((await f.send("gui.find", { name: "input" })).status, "ok");
});

test("named application endpoints have isolated scope without a browser requirement", async () => {
  const firstScope = { window: "node-main", epoch: "application-1", session: "task-1" };
  const secondScope = { ...firstScope, window: "worker-2" };
  const first = new Bridge(firstScope);
  const second = new Bridge(secondScope);
  first.register("alpha", { read: () => scalar("button") });
  second.register("beta", { read: () => scalar("button") });
  const request = { transport_version: 1, ...firstScope, request_id: 1, budget_ms: 1000, operation: "gui.list", arguments: {} };
  assert.equal((await second.handle(request)).error, "wrong_window");
  assert.deepEqual((await first.handle(request)).value, ["alpha"]);
  assert.deepEqual((await second.handle({ ...request, window: secondScope.window })).value, ["beta"]);
  for (const window of ["", "x".repeat(65), "node/main", "node main"]) {
    assert.throws(() => new Bridge({ ...firstScope, window }), /malformed_request/);
  }
});
