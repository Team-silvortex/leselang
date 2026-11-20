import test from "node:test";
import assert from "node:assert/strict";
import { createFormView } from "../dist/form/view.js";
import { Bridge } from "../dist/bridge.js";

class Element extends EventTarget {
  constructor(id) {
    super(); this.id = id;
    this.tagName = id === "input" ? "TEXTAREA" : ["save", "other"].includes(id) ? "BUTTON" : "SPAN";
  }
  type = "button";
  isConnected = true;
  disabled = false;
  readOnly = false;
  textContent = "";
  value = "";
  dataset = {};
  afterClick;
  dispatchEvent(event) {
    const result = super.dispatchEvent(event);
    if (event.type === "click") this.afterClick?.();
    return result;
  }
  click() {
    if (this.disabled) return;
    this.dispatchEvent(new Event("click"));
  }
}

function connection(session) {
  const scope = { window: "main", epoch: "form-page", session };
  const bridge = new Bridge(scope);
  let request = 0;
  return { bridge, call: (operation, arguments_ = {}) => bridge.handle({ transport_version: 1,
    ...scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ }) };
}

async function withForm(run) {
  const previous = globalThis.document;
  const nodes = new Map();
  const node = (id) => { const value = new Element(id); nodes.set(id, value); return value; };
  const elements = Object.fromEntries(["input", "save", "other", "saved", "saveCount", "otherCount", "status"]
    .map((id) => [id, node(id)]));
  elements.save.textContent = "Save";
  elements.other.textContent = "Other control";
  globalThis.document = { defaultView: { Event }, getElementById(id) { return nodes.get(id); } };
  for (const element of nodes.values()) element.ownerDocument = globalThis.document;
  const view = createFormView(elements);
  const { bridge, call } = connection("form-task");
  const observed = (identifier) => view.observe().find((row) => row.identifier === identifier);
  const input = (text) => {
    elements.input.value = text;
    elements.input.dispatchEvent(new Event("input"));
  };
  try { await run({ view, elements, nodes, bridge, call, observed, input }); }
  finally { bridge.close(); view.dispose(); globalThis.document = previous; }
}

async function target(call, name) {
  const target = (await call("gui.find", { name })).value;
  const observed = (await call("gui.observe", { target })).value;
  return { target, expected_revision: observed.revision };
}

test("registered form controls use the same manual input, save and other handlers", async () => {
  await withForm(async ({ view, elements, bridge, call, observed, input }) => {
    input("manual draft");
    elements.save.click();
    await Promise.resolve();
    assert.equal(observed("save").value, "manual draft");
    assert.equal(observed("save").clicks, 1);
    view.attach(bridge);
    assert.deepEqual((await call("gui.list")).value, ["input", "save", "other"]);
    assert.equal((await call("gui.set_text", { ...await target(call, "input"), text: "agent draft ✓" })).status, "ok");
    const prior = view.observe();
    assert.equal(elements.input.value, "agent draft ✓");
    assert.equal((await call("gui.click", await target(call, "save"))).status, "ok");
    assert.equal(observed("save").value, "agent draft ✓");
    assert.equal(observed("save").clicks, 2);
    assert.equal(elements.saved.textContent, "agent draft ✓");
    assert.equal(elements.saveCount.textContent, "2 saves");
    assert.equal(prior.find((row) => row.identifier === "save").value, "manual draft", "observations own their values");
    assert.equal((await call("gui.click", await target(call, "other"))).status, "ok");
    assert.equal(observed("other").clicks, 1);
    view.detach(bridge);
    elements.other.click();
    assert.equal(observed("other").clicks, 2, "manual events still work after detaching");
  });
});

test("manual ABA draft edits invalidate a previously observed save control", async () => {
  await withForm(async ({ view, bridge, call, observed, input }) => {
    input("original");
    view.attach(bridge);
    const action = await target(call, "save");
    input("temporary");
    input("original");
    const reply = await call("gui.click", action);
    assert.equal(reply.status, "rejected");
    assert.equal(reply.error, "stale_revision");
    assert.equal(observed("save").clicks, 0);
    view.detach(bridge);
  });
});

test("a detached form action cannot save or invalidate its replacement session", async () => {
  await withForm(async ({ view, elements, bridge, call, observed, input }) => {
    input("retained draft");
    view.attach(bridge);
    const action = await target(call, "save");
    const next = connection("replacement-task");
    let nextAction;
    elements.save.afterClick = () => {
      view.detach(bridge);
      view.attach(next.bridge);
      nextAction = target(next.call, "save");
    };
    try {
      assert.notEqual((await call("gui.click", action)).status, "ok");
      const replacement = await nextAction;
      await Promise.resolve();
      assert.equal(observed("save").value, "");
      assert.equal(observed("save").clicks, 0);
      const current = (await next.call("gui.observe", { target: replacement.target })).value;
      assert.equal(current.revision, replacement.expected_revision, "old completion did not notify the new bridge");
      elements.save.afterClick = undefined;
      view.detach(bridge);
      assert.equal((await next.call("gui.click", replacement)).status, "ok", "late old detach preserves the replacement");
      assert.equal(observed("save").clicks, 1);
      assert.equal(observed("save").value, "retained draft");
    } finally { view.detach(next.bridge); }
  });
});

test("revocation while the form save is pending retains the draft without publishing", async () => {
  await withForm(async ({ view, elements, bridge, call, observed, input }) => {
    input("revoked draft");
    view.attach(bridge);
    elements.save.afterClick = () => bridge.revoke();
    const reply = await call("gui.click", await target(call, "save"));
    assert.notEqual(reply.status, "ok");
    await Promise.resolve();
    assert.equal(observed("input").value, "revoked draft");
    assert.equal(observed("save").value, "");
    assert.equal(observed("save").clicks, 0);
    view.detach(bridge);
  });
});

test("replaced buttons or tampered input after save starts prevent its late domain mutation", async () => {
  for (const mutate of [
    ({ elements, nodes }) => {
      elements.save.isConnected = false;
      nodes.set("save", new Element("save"));
    },
    ({ elements, nodes }) => {
      elements.other.isConnected = false;
      nodes.set("other", new Element("other"));
    },
    ({ elements }) => { elements.input.value = "unreported edit"; },
  ]) await withForm(async (fixture) => {
    const { view, elements, bridge, call, input } = fixture;
    input("pending draft");
    view.attach(bridge);
    elements.save.afterClick = () => mutate(fixture);
    const reply = await call("gui.click", await target(call, "save"));
    assert.notEqual(reply.status, "ok");
    await Promise.resolve();
    assert.equal(elements.saved.textContent, "");
    assert.equal(elements.saveCount.textContent, "0 saves");
    assert.throws(() => view.observe(), /DOM instance has been replaced|DOM and application state differ/u);
    view.detach(bridge);
  });
});

test("replaced DOM and mismatched readback fail observation and partial attachment closes the bridge", async () => {
  await withForm(async ({ view, elements, nodes, bridge, call, observed }) => {
    elements.save.isConnected = false;
    nodes.set("save", new Element("save"));
    assert.throws(() => view.observe(), /DOM instance has been replaced/u);
    assert.throws(() => view.attach(bridge), /DOM instance has been replaced/u);
    assert.equal((await call("gui.list")).status, "rejected", "earlier input registration cannot remain active");
    elements.save.isConnected = true;
    nodes.set("save", elements.save);
    const next = connection("recovered-task");
    try {
      view.attach(next.bridge);
      assert.equal(observed("save").clicks, 0);
      elements.saved.textContent = "forged";
      assert.throws(() => view.observe(), /DOM and application state differ/u);
    } finally { view.detach(next.bridge); }
  });
});
