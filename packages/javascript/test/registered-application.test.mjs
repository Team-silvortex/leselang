import assert from "node:assert/strict";
import test from "node:test";
import { Bridge, createRegisteredApplication } from "../dist/index.js";

const state = (value = "", options = {}) => ({ role: "button", title: "Control", value,
  enabled: true, editable: false, clicks: 0, ...options });
let serial = 0;
function bridge() { return new Bridge({ window: "application", epoch: "page", session: `run_${++serial}` }); }
function carrier(current) {
  let request = 0;
  return (operation, args = {}) => current.handle({ transport_version: 1, ...current.scope,
    request_id: ++request, budget_ms: 1000, operation, arguments: args });
}
async function target(call, name) {
  const found = await call("gui.find", { name });
  assert.equal(found.status, "ok");
  const observed = await call("gui.observe", { target: found.value });
  assert.equal(observed.status, "ok");
  return { target: found.value, expected_revision: observed.value.revision };
}
function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

test("application declarations are bounded, fixed and admitted without invoking accessors or readers", () => {
  let calls = 0;
  const valid = { read() { calls++; return state(); } };
  const declarations = { item: valid };
  const app = createRegisteredApplication(declarations);
  assert.equal(calls, 0);
  declarations.later = valid;
  declarations.item = { read() { throw new Error("replaced declaration"); } };
  valid.read = () => { throw new Error("replaced function"); };
  assert.deepEqual(app.observe().map((row) => row.identifier), ["item"]);
  assert.equal(calls, 1);
  const accessor = {};
  Object.defineProperty(accessor, "read", { enumerable: true, get() { calls++; return () => state(); } });
  const mapAccessor = {};
  Object.defineProperty(mapAccessor, "item", { enumerable: true, get() { calls++; return valid; } });
  for (const value of [null, [], 1, { "bad name": valid }, { "": valid },
    { ["a".repeat(65)]: valid }, { [Symbol("control")]: valid }, { item: null }, { item: {} },
    { item: { read: 1 } }, { item: { read() {}, click: "not a function" } },
    { item: accessor }, mapAccessor, Object.fromEntries(Array.from({ length: 65 }, (_, i) => [`c${i}`, valid]))]) {
    assert.throws(() => createRegisteredApplication(value), /invalid_registered_application_controls/);
  }
  assert.equal(calls, 1, "accessor admission performs no user callback");
});

test("observations read live state before attach and after detach and own the returned records", () => {
  const model = state("before");
  const app = createRegisteredApplication({ z: { read: () => model }, a: { read: () => state("other") } });
  const before = app.observe();
  assert.deepEqual(before.map((row) => row.identifier), ["z", "a"]);
  const current = bridge(); app.attach(current);
  model.value = "after";
  assert.equal(app.observe()[0].value, "after");
  app.detach(current);
  model.value = "detached";
  const after = app.observe();
  assert.equal(after[0].value, "detached");
  after[0].value = "forged";
  assert.equal(app.observe()[0].value, "detached");
  assert.equal(before[0].value, "before");
});

test("independent observations reject malformed state and never evaluate evidence getters", () => {
  let accesses = 0;
  const getter = state();
  Object.defineProperty(getter, "value", { enumerable: true, get() { accesses++; return "forged"; } });
  for (const value of [getter, { ...state(), clicks: -1 }, { ...state(), clicks: 2 ** 32 },
    { ...state(), enabled: "yes" }, { ...state(), role: "x".repeat(65) },
    { ...state(), value: "\ud800" }, { ...state(), value: "x".repeat(4097) },
    { ...state(), identifier: "forged" }, { ...state(), extra: true },
    { ...state(), [Symbol("hidden")]: "extra" }, new (class { value = "wrong prototype"; })()]) {
    const app = createRegisteredApplication({ item: { read: () => value } });
    assert.throws(() => app.observe(), /invalid_application_(evidence|observation)/);
  }
  assert.equal(accesses, 0);
});

test("an empty application can attach but a closed bridge cannot attach", () => {
  const app = createRegisteredApplication({});
  const closed = bridge(); closed.close();
  assert.equal(closed.closed, true);
  assert.throws(() => app.attach(closed), /detached/);
  const current = bridge(); app.attach(current);
  assert.deepEqual(app.observe(), []);
  assert.throws(() => app.attach(bridge()), /already_attached/);
  app.detach(current);
  assert.equal(current.closed, true);
  const next = bridge(); app.attach(next); app.detach(next);
});

test("reader reentry cannot attach a second bridge before the first attachment completes", () => {
  const current = bridge(), other = bridge();
  let app;
  let reads = 0;
  app = createRegisteredApplication({ item: { read() {
    reads++;
    assert.throws(() => app.attach(other), /already_attached/);
    return state();
  } } });
  app.attach(current);
  assert.equal(reads, 1);
  assert.equal(other.closed, false, "a refused contender retains its own lifecycle");
  app.detach(current); other.close();
});

for (const reason of ["throw", "close", "detach"]) {
  test(`partial attachment ${reason} retires the old bridge and releases the application`, async () => {
    const current = bridge(), other = bridge();
    let app, failing = true;
    app = createRegisteredApplication({ first: { read: () => state() }, last: { read() {
      if (failing) {
        if (reason === "throw") throw new Error("reader failed");
        if (reason === "close") current.close();
        if (reason === "detach") {
          app.detach(current);
          assert.throws(() => app.attach(other), /already_attached/, "detach during read retains the attachment slot");
        }
      }
      return state();
    } } });
    assert.throws(() => app.attach(current));
    assert.equal(current.closed, true);
    assert.equal((await carrier(current)("gui.list")).status, "rejected");
    failing = false;
    app.attach(other);
    app.detach(current);
    assert.equal((await carrier(other)("gui.list")).status, "ok", "old detach cannot retire the next session");
    app.detach(other);
  });
}

test("Bridge register cannot resurrect a closed bridge or overwrite a reentrant registration", async () => {
  const closed = bridge();
  assert.throws(() => closed.register("item", { read() { closed.close(); return state(); } }), /closed/);
  const current = bridge();
  assert.throws(() => current.register("item", { read() {
    current.register("item", { read: () => state("inner") });
    return state("outer");
  } }), /stale_object/);
  const call = carrier(current), item = await target(call, "item");
  assert.equal((await call("gui.observe", { target: item.target })).value.value, "inner");
  current.close();
});

test("manual changes fence invisible dependencies and ABA without partially accepting unknown names", async () => {
  let draft = "original";
  const app = createRegisteredApplication({
    draft: { read: () => state(draft, { role: "text_field", editable: true }) },
    save: { read: () => state("saved") },
  });
  assert.doesNotThrow(() => app.changed("draft", "save"));
  assert.throws(() => app.changed("unknown"), /unknown_registered_control/);
  const current = bridge(); app.attach(current); const call = carrier(current);
  const old = await target(call, "save");
  assert.throws(() => app.changed("save", "unknown"), /unknown_registered_control/);
  assert.equal((await call("gui.observe", { target: old.target })).value.revision, old.expected_revision);
  draft = "temporary"; app.changed("draft", "save");
  draft = "original"; app.changed("draft", "save");
  const after = (await call("gui.observe", { target: old.target })).value;
  assert.equal(after.value, "saved");
  assert.equal(after.revision, old.expected_revision + 2);
  const stale = await call("gui.click", old);
  assert.equal(stale.error, "stale_revision");
  app.detach(current);
});

test("explicit action commits notify dependencies and permit the action to disable itself", async () => {
  let done = false, saved = "before";
  const app = createRegisteredApplication({
    action: { read: () => state(done ? "done" : "pending", { enabled: !done, clicks: Number(done) }),
      click(context) { context.commit(() => { done = true; saved = "after"; context.changed("action", "saved"); }); } },
    saved: { read: () => state(saved) },
  });
  const current = bridge(); app.attach(current); const call = carrier(current);
  const savedBefore = await target(call, "saved"), action = await target(call, "action");
  assert.equal((await call("gui.click", action)).status, "ok");
  const after = (await call("gui.observe", { target: action.target })).value;
  assert.equal(after.enabled, false);
  assert.equal(after.clicks, 1);
  assert.equal((await call("gui.observe", { target: savedBefore.target })).value.value, "after");
  assert.equal((await call("gui.click", { target: action.target, expected_revision: after.revision })).error, "disabled");
  app.detach(current);
});

test("dependency notification requires the explicit synchronous commit and validates every name first", async () => {
  let mode = "outside";
  const app = createRegisteredApplication({
    action: { read: () => state(), click(context) {
      if (mode === "outside") context.changed("dependency");
      else context.commit(() => context.changed("dependency", "unknown"));
    } },
    dependency: { read: () => state() },
  });
  const current = bridge(); app.attach(current); const call = carrier(current);
  const dep = await target(call, "dependency");
  for (mode of ["outside", "invalid"]) {
    const reply = await call("gui.click", await target(call, "action"));
    assert.equal(reply.status, "uncertain");
    assert.equal(reply.error, "adapter_failed");
    assert.equal((await call("gui.observe", { target: dep.target })).value.revision, dep.expected_revision);
  }
  app.detach(current);
});

for (const stop of ["cancel", "revoke", "detach"]) {
  test(`${stop} fences a late action commit and cannot notify the next session`, async () => {
    const waiting = deferred(), entered = deferred();
    let model = "before", captured;
    const app = createRegisteredApplication({
      action: { read: () => state(model), async click(context) {
        captured = context; entered.resolve(); await waiting.promise;
        context.commit(() => { model = "late"; context.changed("dependency"); });
      } },
      dependency: { read: () => state() },
    });
    const current = bridge(); app.attach(current); const call = carrier(current);
    const pending = call("gui.click", await target(call, "action")); await entered.promise;
    if (stop === "cancel") current.cancel(current.scope.session);
    if (stop === "revoke") current.revoke();
    if (stop === "detach") app.detach(current);
    assert.equal(captured.signal.aborted, true);
    assert.equal((await pending).status, "uncertain");
    app.detach(current);
    const next = bridge(); app.attach(next); const nextCall = carrier(next);
    const dep = await target(nextCall, "dependency");
    waiting.resolve(); await Promise.resolve(); await Promise.resolve();
    assert.equal(model, "before");
    assert.throws(() => captured.commit(() => { model = "late"; }), /detached/);
    assert.throws(() => captured.changed("dependency"), /outside_commit/);
    assert.equal((await nextCall("gui.observe", { target: dep.target })).value.revision, dep.expected_revision);
    app.detach(next);
  });
}

test("revoke within commit blocks later dependency notification but retains earlier partial mutation", async () => {
  let model = "before", notificationError;
  const current = bridge();
  const app = createRegisteredApplication({
    action: { read: () => state(model), click(context) { context.commit(() => {
      model = "committed"; current.revoke();
      try { context.changed("dependency"); } catch (error) { notificationError = error; throw error; }
    }); } },
    dependency: { read: () => state() },
  });
  app.attach(current); const call = carrier(current);
  const reply = await call("gui.click", await target(call, "action"));
  assert.equal(reply.status, "uncertain");
  assert.match(notificationError.message, /registered_action_cancelled/);
  assert.equal(app.observe()[0].value, "committed");
  app.detach(current);
});

test("a throwing mutation keeps partial effects without retry or rollback", async () => {
  let writes = 0;
  const app = createRegisteredApplication({ action: {
    read: () => state(String(writes), { clicks: writes }),
    click(context) { context.commit(() => { writes++; throw new Error("after mutation"); }); },
  } });
  const current = bridge(); app.attach(current); const call = carrier(current);
  const reply = await call("gui.click", await target(call, "action"));
  assert.equal(reply.status, "uncertain");
  assert.equal(writes, 1);
  app.detach(current);
  assert.equal(app.observe()[0].clicks, 1);
});

test("async commits and reentrant commits retain the underlying bridge rejection", async () => {
  let mode = "async";
  const app = createRegisteredApplication({ action: { read: () => state(), click(context) {
    if (mode === "async") context.commit(async () => {});
    else context.commit(() => context.commit(() => {}));
  } } });
  const current = bridge(); app.attach(current); const call = carrier(current);
  for (mode of ["async", "reentrant"]) {
    const reply = await call("gui.click", await target(call, "action"));
    assert.equal(reply.status, "uncertain");
    assert.equal(reply.error, mode === "async" ? "async_commit" : "reentrant_commit");
  }
  app.detach(current);
});

for (const stop of ["revoke", "close", "replace", "unregister"]) {
  test(`a reader's reentrant ${stop} fences the underlying Bridge before commit mutation`, async () => {
    const current = bridge();
    let armed = false, writes = 0;
    current.register("action", {
      read() {
        if (armed) {
          armed = false;
          if (stop === "revoke") current.revoke();
          if (stop === "close") current.close();
          if (stop === "replace") current.replace("action", { read: () => state() });
          if (stop === "unregister") current.unregister("action");
        }
        return state();
      },
      click(context) { armed = true; context.commit(() => { writes++; }); },
    });
    const call = carrier(current);
    const reply = await call("gui.click", await target(call, "action"));
    assert.equal(reply.status, "uncertain");
    assert.equal(writes, 0, "authority and identity must still hold after the reader returns");
    current.close();
  });
}

test("a reentrant replacement cannot reuse its inner generation or redirect the inner target", async () => {
  const current = bridge();
  current.register("item", { read: () => state("original") });
  const call = carrier(current);
  let innerTarget;
  assert.throws(() => current.replace("item", { read() {
    current.replace("item", { read: () => state("inner") });
    innerTarget = call("gui.find", { name: "item" });
    return state("outer");
  } }), /stale_object/);
  const found = await innerTarget;
  assert.equal(found.status, "ok");
  assert.equal(found.value.generation, 2);
  const observed = await call("gui.observe", { target: found.value });
  assert.equal(observed.status, "ok");
  assert.equal(observed.value.value, "inner");
  current.close();
});

test("an observation whose reader replaces or unregisters itself cannot return an obsolete snapshot", async () => {
  for (const change of ["replace", "unregister"]) {
    const current = bridge();
    let armed = false;
    current.register("item", { read() {
      if (armed) {
        armed = false;
        if (change === "replace") current.replace("item", { read: () => state("replacement") });
        else current.unregister("item");
      }
      return state("obsolete");
    } });
    const call = carrier(current), item = await target(call, "item");
    armed = true;
    const reply = await call("gui.observe", { target: item.target });
    assert.equal(reply.status, "rejected");
    assert.equal(reply.error, "stale_object");
    assert.equal(reply.value, null);
    current.close();
  }
});
