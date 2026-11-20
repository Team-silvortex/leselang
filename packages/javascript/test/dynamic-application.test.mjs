import assert from "node:assert/strict";
import test from "node:test";
import { Bridge, createRegisteredApplication, createDynamicRegisteredApplication } from "@leselang/bridge";

const state = (value = "same", extra = {}) => ({ role: "button", title: "Control", value,
  enabled: true, editable: false, clicks: 0, ...extra });
const control = (value = "same") => ({ read: () => state(value) });
let serial = 0;
function connection() {
  const bridge = new Bridge({ window: "dynamic", epoch: "page", session: `run_${++serial}` });
  let request = 0;
  return { bridge, call: (operation, arguments_ = {}) => bridge.handle({ transport_version: 1,
    ...bridge.scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ }) };
}
async function target(call, name) {
  const found = await call("gui.find", { name });
  assert.equal(found.status, "ok", found.error);
  const observed = await call("gui.observe", { target: found.value });
  assert.equal(observed.status, "ok", observed.error);
  return { target: found.value, expected_revision: observed.value.revision };
}
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}

test("dynamic slots are admitted without reading, own methods and preserve the static return shape", () => {
  let reads = 0, getters = 0;
  const initial = { read() { reads++; return state(); } }, slots = { late: null, first: initial };
  const app = createDynamicRegisteredApplication(slots);
  assert.equal(reads, 0);
  assert.equal(app.bindingRevision(), 0);
  assert.ok(Object.isFrozen(app));
  initial.read = () => { throw new Error("changed method"); };
  slots.first = null; slots.extra = control();
  assert.deepEqual(app.observe().map((row) => row.identifier), ["first"]);
  assert.equal(reads, 1);
  const accessor = { get read() { getters++; return () => state(); } };
  for (const invalid of [null, [], { name: undefined }, { name: accessor },
    { get name() { getters++; return control(); } }, { "bad name": null },
    Object.fromEntries(Array.from({ length: 65 }, (_, index) => [`slot${index}`, null]))]) {
    assert.throws(() => createDynamicRegisteredApplication(invalid), /invalid_registered_application_controls/);
  }
  assert.equal(getters, 0);
  const fixed = createRegisteredApplication({ fixed: control() });
  assert.deepEqual(Object.keys(fixed), ["attach", "detach", "observe", "changed"]);
  assert.equal(Object.hasOwn(fixed, "bindingRevision"), false);
  assert.throws(() => createRegisteredApplication({ empty: null }));
});

test("only successful explicit structural changes increment binding revision", () => {
  const app = createDynamicRegisteredApplication({ first: null, second: control("old") });
  const expectRevision = (expected) => assert.equal(app.bindingRevision(), expected);
  app.mount("first", control("mounted")); expectRevision(1);
  app.replace("second", control("replacement")); expectRevision(2);
  assert.deepEqual(app.observe().map((row) => [row.identifier, row.value]), [["first", "mounted"], ["second", "replacement"]]);
  app.unmount("first"); expectRevision(3);
  for (const change of [
    () => app.mount("unknown", control()), () => app.replace("unknown", control()), () => app.unmount("unknown"),
    () => app.mount("second", control()), () => app.replace("first", control()), () => app.unmount("first"),
    () => app.mount("first", null), () => app.replace("second", { read: "bad" }),
  ]) { assert.throws(change); expectRevision(3); }
  app.changed("first", "second"); expectRevision(3);
  const current = connection(); app.attach(current.bridge); expectRevision(3);
  app.detach(current.bridge); expectRevision(3);
  app.mount("first", control("again")); expectRevision(4);
});

test("same-value replacement and unmount/remount invalidate old handles while preserving other slots", async () => {
  const app = createDynamicRegisteredApplication({ item: control(), other: control("other"), late: null });
  const { bridge, call } = connection(); app.attach(bridge);
  const first = await target(call, "item"), other = await target(call, "other");
  app.replace("item", control());
  const second = await target(call, "item");
  assert.ok(second.target.generation > first.target.generation);
  assert.equal((await call("gui.observe", { target: first.target })).error, "stale_object");
  app.unmount("item");
  assert.equal((await call("gui.find", { name: "item" })).error, "unknown_object");
  app.mount("item", control());
  const third = await target(call, "item");
  assert.ok(third.target.generation > second.target.generation);
  assert.equal((await call("gui.observe", { target: second.target })).error, "stale_object");
  assert.equal((await call("gui.observe", { target: other.target })).value.revision, other.expected_revision);
  app.mount("late", control("new"));
  assert.deepEqual((await call("gui.list")).value, ["item", "other", "late"]);
  app.detach(bridge);
});

test("declared empty dependencies are skipped and unknown notifications remain atomic", async () => {
  let text = "before";
  const app = createDynamicRegisteredApplication({
    input: { read: () => state(text, { role: "text_field", editable: true }),
      setText(value, context) { context.commit(() => { text = value; context.changed("input", "publish"); }); } },
    publish: null,
  });
  const { bridge, call } = connection(); app.attach(bridge);
  const input = await target(call, "input");
  assert.throws(() => app.changed("input", "unknown"), /unknown_registered_control/);
  assert.equal((await call("gui.observe", { target: input.target })).value.revision, input.expected_revision);
  app.changed("publish");
  assert.equal((await call("gui.set_text", { ...input, text: "updated" })).status, "ok");
  assert.equal(app.bindingRevision(), 0);
  assert.equal(app.observe()[0].value, "updated");
  app.mount("publish", control("ready"));
  assert.equal(app.bindingRevision(), 1);
  assert.equal((await call("gui.find", { name: "publish" })).status, "ok");
  app.detach(bridge);
});

for (const change of ["same", "remove", "dependency"]) {
  test(`a pending action cannot commit through ${change} structural replacement`, async () => {
    const entered = deferred(), release = deferred();
    let writes = 0, captured;
    const app = createDynamicRegisteredApplication({ action: {
      read: () => state(String(writes)),
      async click(context) { captured = context; entered.resolve(); await release.promise;
        context.commit(() => { writes++; context.changed("dependency"); }); },
    }, dependency: control() });
    const { bridge, call } = connection(); app.attach(bridge);
    const pending = call("gui.click", await target(call, "action")); await entered.promise;
    if (change === "same") app.replace("action", control("0"));
    if (change === "remove") { app.unmount("action"); app.mount("action", control("0")); }
    if (change === "dependency") app.replace("dependency", control());
    release.resolve();
    assert.equal((await pending).status, "uncertain");
    assert.equal(writes, 0);
    assert.throws(() => captured.commit(() => { writes++; }), /bindings_changed/);
    assert.equal(writes, 0);
    app.detach(bridge);
  });
}

test("structure changed inside a commit preserves its first effect but cannot notify the new instance", async () => {
  let writes = 0;
  const app = createDynamicRegisteredApplication({
    action: { read: () => state(String(writes)), click(context) { context.commit(() => {
      writes++; app.replace("dependency", control()); context.changed("dependency");
    }); } }, dependency: control(),
  });
  const { bridge, call } = connection(); app.attach(bridge);
  const old = await target(call, "dependency");
  assert.equal((await call("gui.click", await target(call, "action"))).status, "uncertain");
  const next = await target(call, "dependency");
  assert.ok(next.target.generation > old.target.generation);
  assert.equal(next.expected_revision, 0, "old context did not notify the replacement");
  assert.equal(writes, 1);
  app.detach(bridge);
});

test("observe and adapter readers cannot mutate slots or reenter observation", async () => {
  let app, armed = false;
  app = createDynamicRegisteredApplication({ item: { read() {
    if (armed) app.replace("other", control("new"));
    return state();
  } }, other: control("old") });
  armed = true;
  assert.throws(() => app.observe(), /registered_application_busy/);
  assert.equal(app.bindingRevision(), 0);
  armed = false;
  const { bridge, call } = connection(); app.attach(bridge);
  const item = await target(call, "item"); armed = true;
  assert.equal((await call("gui.observe", { target: item.target })).status, "rejected");
  assert.equal(app.bindingRevision(), 0);
  armed = false;
  assert.equal(app.observe().find((row) => row.identifier === "other").value, "old");
  app.detach(bridge);
  let reentrant;
  reentrant = createDynamicRegisteredApplication({ item: { read() { return reentrant.observe()[0]; } } });
  assert.throws(() => reentrant.observe(), /registered_application_busy/);
});

for (const mode of ["throw", "close", "detach", "reenter"]) {
  test(`failed live replacement ${mode} retires its bridge and preserves recoverable declarations`, async () => {
    const app = createDynamicRegisteredApplication({ item: control("old"), empty: null });
    const current = connection(); app.attach(current.bridge);
    assert.throws(() => app.replace("item", { read() {
      if (mode === "throw") throw new Error("reader failed");
      if (mode === "close") current.bridge.close();
      if (mode === "detach") app.detach(current.bridge);
      if (mode === "reenter") app.mount("empty", control());
      return state("new");
    } }));
    assert.equal(current.bridge.closed, true);
    assert.equal(app.bindingRevision(), 0);
    assert.deepEqual(app.observe().map((row) => [row.identifier, row.value]), [["item", "old"]]);
    const next = connection(); app.attach(next.bridge);
    app.detach(current.bridge);
    assert.equal((await next.call("gui.find", { name: "item" })).status, "ok");
    app.detach(next.bridge);
  });
}

test("malformed replacement declarations do not retire an otherwise usable attachment", async () => {
  let getters = 0;
  const app = createDynamicRegisteredApplication({ item: control() });
  const { bridge, call } = connection(); app.attach(bridge);
  assert.throws(() => app.replace("item", { get read() { getters++; return () => state(); } }), /invalid_registered_application_controls/);
  assert.equal(getters, 0);
  assert.equal(bridge.closed, false);
  assert.equal(app.bindingRevision(), 0);
  assert.equal((await call("gui.find", { name: "item" })).status, "ok");
  app.detach(bridge);
});

test("all sixty-four declared slots can be reused without growing the bridge name history", async () => {
  const app = createDynamicRegisteredApplication(Object.fromEntries(Array.from({ length: 64 }, (_, index) => [`s${index}`, null])));
  const { bridge, call } = connection(); app.attach(bridge);
  for (let index = 0; index < 64; index++) { app.mount(`s${index}`, control()); app.unmount(`s${index}`); }
  for (let index = 0; index < 10; index++) { app.mount("s0", control()); app.unmount("s0"); }
  app.mount("s0", control());
  assert.deepEqual((await call("gui.list")).value, ["s0"]);
  assert.equal(app.bindingRevision(), 149);
  assert.throws(() => app.mount("extra", control()), /unknown_registered_control/);
  assert.equal(app.bindingRevision(), 149);
  app.detach(bridge);
});
