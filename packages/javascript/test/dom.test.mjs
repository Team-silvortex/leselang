import assert from "node:assert/strict";
import test from "node:test";
import { Bridge, createRegisteredApplication } from "@leselang/bridge";
import { createDomButtonControl, createDomTextControl } from "../dist/dom/index.js";

// A deliberately small EventTarget fixture; native WebView behavior is accepted
// separately. No DOM globals or browser dependency are installed by this test.
class Element extends EventTarget {
  constructor(id, tagName, ownerDocument) { super(); Object.assign(this, { id, tagName, ownerDocument }); }
  isConnected = true;
  disabled = false;
  readOnly = false;
  type = "button";
  value = "";
}
function deferred() { let resolve; const promise = new Promise((yes) => { resolve = yes; }); return { promise, resolve }; }
function setup(options = {}) {
  const nodes = new Map();
  const document = { getElementById: (id) => nodes.get(id), defaultView: { Event } };
  const input = new Element("input", "TEXTAREA", document), button = new Element("save", "BUTTON", document);
  nodes.set(input.id, input); nodes.set(button.id, button);
  options.beforeBinding?.({ input, button });
  const state = { draft: "", saved: "", clicks: 0 }, errors = [], contexts = [];
  let application, inputs = 0, clicks = 0, validations = 0;
  const changed = (...names) => application.changed(...names);
  const onError = (error, origin) => { errors.push({ error, origin }); options.onError?.(error, origin); };
  const text = createDomTextControl({ element: input, changed, onError,
    read() {
      if (input.value !== state.draft) throw new Error("input mismatch");
      return { role: "text_field", title: "Draft", value: input.value, enabled: !input.disabled, editable: !input.readOnly, clicks: 0 };
    },
    validateText(value) { validations++; if (value === "invalid") throw new Error("bad text"); },
    onInput(value, context) {
      inputs++; options.input?.(value, context);
      state.draft = value; context.changed("input", "save");
    },
  });
  const save = createDomButtonControl({ element: button, changed, onError,
    read: () => ({ role: "button", title: "Save", value: state.saved, enabled: !button.disabled, editable: false, clicks: state.clicks }),
    beforeCommit: () => options.beforeCommit?.({ input, button, state }),
    async onClick(context) {
      contexts.push(context); clicks++;
      await options.beforeClick?.(clicks, context);
      context.commit(() => { state.saved = state.draft; state.clicks++; context.changed("save"); options.committed?.({ button }); });
      options.afterCommit?.();
    },
  });
  application = createRegisteredApplication({ input: text.control, save: save.control });
  const scope = { window: "dom", epoch: "test", session: "run" };
  const bridge = new Bridge(scope);
  let serial = 0;
  const call = (operation, args = {}) => bridge.handle({ transport_version: 1, ...scope,
    request_id: ++serial, budget_ms: 1000, operation, arguments: args });
  const target = async (name) => {
    const target = (await call("gui.find", { name })).value;
    return { target, expected_revision: (await call("gui.observe", { target })).value.revision };
  };
  return { application, input, button, nodes, document, state, errors, contexts, bridge, call, target, text, save,
    counts: () => ({ inputs, clicks, validations }),
    inputManually(value) { input.value = value; input.dispatchEvent(new Event("input", { bubbles: true, cancelable: true })); },
    clickManually() { button.dispatchEvent(new Event("click", { bubbles: true, cancelable: true })); },
    close() { application.detach(bridge); text.dispose(); save.dispose(); } };
}

test("DOM bindings import without globals and share manual and agent handlers", async () => {
  assert.equal(typeof globalThis.document, "undefined");
  const f = setup();
  try {
    assert.deepEqual(f.counts(), { inputs: 0, clicks: 0, validations: 0 }, "construction never reads or runs domain handlers");
    f.inputManually("manual"); f.clickManually(); await Promise.resolve(); await Promise.resolve();
    assert.deepEqual(f.state, { draft: "manual", saved: "manual", clicks: 1 });
    f.application.attach(f.bridge);
    assert.equal((await f.call("gui.set_text", { ...await f.target("input"), text: "agent" })).status, "ok");
    assert.equal((await f.call("gui.click", await f.target("save"))).status, "ok");
    assert.deepEqual(f.state, { draft: "agent", saved: "agent", clicks: 2 });
    assert.deepEqual(f.counts(), { inputs: 2, clicks: 2, validations: 2 });
    assert.equal(f.errors.length, 0);
    assert.ok(Object.isFrozen(f.text) && Object.isFrozen(f.text.control));
  } finally { f.close(); }
});

test("text validation happens before agent DOM assignment and input failure preserves throw undefined", async () => {
  const f = setup({ input(value) { if (value === "throw") throw undefined; }, onError() { throw new Error("display failed"); } });
  try {
    f.inputManually("prior"); f.application.attach(f.bridge);
    const invalid = await f.call("gui.set_text", { ...await f.target("input"), text: "invalid" });
    assert.notEqual(invalid.status, "ok");
    assert.equal(f.input.value, "prior"); assert.equal(f.state.draft, "prior");
    const thrown = await f.call("gui.set_text", { ...await f.target("input"), text: "throw" });
    assert.notEqual(thrown.status, "ok");
    assert.equal(f.state.draft, "prior");
    assert.equal(f.input.value, "throw", "DOM mutation is not automatically rolled back");
    assert.equal(f.errors.at(-1).error, undefined);
    assert.equal(f.errors.at(-1).origin, "agent");
  } finally { f.close(); }
});

test("manual ABA changes still stale the save observation", async () => {
  const f = setup();
  try {
    f.inputManually("first"); f.application.attach(f.bridge);
    const saved = await f.target("save");
    f.inputManually("second"); f.inputManually("first");
    const result = await f.call("gui.click", saved);
    assert.equal(result.error, "stale_revision"); assert.equal(f.state.clicks, 0);
  } finally { f.close(); }
});

test("pending button contexts retain revoke, detach, disposal and replacement fences", async () => {
  for (const mode of ["revoke", "detach", "dispose", "replace"]) {
    const entered = deferred(), release = deferred();
    const f = setup({ async beforeClick() { entered.resolve(); await release.promise; } });
    try {
      f.inputManually("retained"); f.application.attach(f.bridge);
      const pending = f.call("gui.click", await f.target("save"));
      await entered.promise;
      if (mode === "revoke") f.bridge.revoke();
      if (mode === "detach") f.application.detach(f.bridge);
      if (mode === "dispose") f.save.dispose();
      if (mode === "replace") f.nodes.set("save", new Element("save", "BUTTON", f.document));
      release.resolve();
      assert.notEqual((await pending).status, "ok", mode);
      assert.deepEqual(f.state, { draft: "retained", saved: "", clicks: 0 }, mode);
    } finally { release.resolve(); f.close(); }
  }
});

test("a manual event during an await never adopts or replaces the agent completion", async () => {
  const entered = deferred(), release = deferred();
  const f = setup({ async beforeClick(count) { if (count === 1) { entered.resolve(); await release.promise; } } });
  try {
    f.inputManually("draft"); f.application.attach(f.bridge);
    const pending = f.call("gui.click", await f.target("save"));
    await entered.promise; f.clickManually(); await Promise.resolve(); await Promise.resolve();
    assert.equal(f.state.clicks, 1);
    assert.notEqual(f.contexts[0].signal, f.contexts[1].signal);
    release.resolve();
    assert.notEqual((await pending).status, "ok", "manual change invalidates the captured agent revision");
    assert.equal(f.state.clicks, 1);
  } finally { release.resolve(); f.close(); }
});

test("unexpected synchronous nested events cannot steal an invocation before its listener runs", async () => {
  let nested = false;
  const f = setup({ beforeBinding({ button }) { button.addEventListener("click", () => {
    if (!nested) { nested = true; button.dispatchEvent(new Event("click")); }
  }); } });
  try {
    f.application.attach(f.bridge);
    assert.notEqual((await f.call("gui.click", await f.target("save"))).status, "ok");
    assert.equal(f.counts().clicks, 0); assert.equal(f.state.clicks, 0);
    assert.match(f.errors[0].error.message, /dom_event_reentry/);
  } finally { f.close(); }
});

test("input event substitutions fail before the domain handler and cannot silently replace the supplied text", async () => {
  const f = setup({ beforeBinding({ input }) { input.addEventListener("input", () => { input.value = "substitution"; }); } });
  try {
    f.application.attach(f.bridge);
    assert.notEqual((await f.call("gui.set_text", { ...await f.target("input"), text: "requested" })).status, "ok");
    assert.equal(f.state.draft, ""); assert.equal(f.counts().inputs, 0);
    assert.match(f.errors[0].error.message, /dom_input_value_changed/);
  } finally { f.close(); }
});

test("synchronous events on another binding cannot turn an agent dispatch into manual authority", async () => {
  const release = deferred();
  const f = setup({ async beforeClick() { await release.promise; } });
  const other = new Element("other", "BUTTON", f.document); f.nodes.set("other", other);
  let otherClicks = 0;
  const otherBinding = createDomButtonControl({ element: other, changed() {},
    read: () => ({ role: "button", title: "Other", value: "", enabled: true, editable: false, clicks: otherClicks }),
    onClick(context) { context.commit(() => { otherClicks++; }); },
  });
  f.button.addEventListener("click", () => other.dispatchEvent(new Event("click")));
  try {
    f.application.attach(f.bridge);
    assert.notEqual((await f.call("gui.click", await f.target("save"))).status, "ok");
    release.resolve(); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    assert.equal(otherClicks, 0); assert.equal(f.state.clicks, 0);
    other.dispatchEvent(new Event("click"));
    assert.equal(otherClicks, 1, "later manual events are independent once the dispatch stack has returned");
  } finally { release.resolve(); otherBinding.dispose(); f.close(); }
});

test("prevented handler dispatch, disabled controls and read-only fields do not claim success", async () => {
  for (const mode of ["stopped", "disabled", "readonly"]) {
    const f = setup(mode === "stopped" ? { beforeBinding({ button }) { button.addEventListener("click", (event) => event.stopImmediatePropagation()); } } : {});
    try {
      if (mode === "disabled") f.button.disabled = true;
      if (mode === "readonly") f.input.readOnly = true;
      f.application.attach(f.bridge);
      const result = mode === "readonly"
        ? await f.call("gui.set_text", { ...await f.target("input"), text: "blocked" })
        : await f.call("gui.click", await f.target("save"));
      assert.notEqual(result.status, "ok", mode);
      assert.equal(f.state.clicks, 0); assert.equal(f.state.draft, "");
    } finally { f.close(); }
  }
});

test("a self-disabling successful button stays accepted, while post-commit faults preserve its effect", async () => {
  for (const fault of [false, true]) {
    const f = setup({ committed({ button }) { button.disabled = true; }, afterCommit() { if (fault) throw new Error("after commit"); } });
    try {
      f.inputManually("retained"); f.application.attach(f.bridge);
      const result = await f.call("gui.click", await f.target("save"));
      assert.equal(result.status === "ok", !fault);
      assert.equal(f.state.clicks, 1); assert.equal(f.state.saved, "retained");
      assert.equal(f.button.disabled, true);
    } finally { f.close(); }
  }
});

test("disposal is idempotent, removes manual listeners and fences previously captured manual contexts", async () => {
  const entered = deferred(), release = deferred();
  const f = setup({ async beforeClick() { entered.resolve(); await release.promise; } });
  try {
    f.inputManually("draft"); f.clickManually(); await entered.promise;
    f.text.dispose(); f.save.dispose(); f.save.dispose();
    assert.equal(f.contexts[0].signal.aborted, true);
    release.resolve(); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    f.inputManually("later"); f.clickManually(); await Promise.resolve();
    assert.equal(f.state.clicks, 0); assert.equal(f.state.draft, "draft");
    assert.throws(() => f.application.observe(), /dom_control_disposed/);
  } finally { release.resolve(); f.close(); }
});

test("duplicate element bindings are refused and disposal permits a new independent binding", () => {
  const f = setup();
  try {
    const options = { element: f.button, read: () => ({ role: "button", title: "New", value: "", enabled: true, editable: false, clicks: 0 }),
      changed() {}, onClick() {} };
    assert.throws(() => createDomButtonControl(options), /dom_control_already_bound/);
    f.save.dispose();
    const next = createDomButtonControl(options);
    assert.equal(next.control.read().title, "New");
    f.save.dispose();
    assert.throws(() => createDomButtonControl(options), /dom_control_already_bound/, "old disposal cannot remove a new binding");
    next.dispose();
  } finally { f.close(); }
});

test("cancellation in an earlier event listener prevents input and button domain handlers", async () => {
  for (const kind of ["input", "button"]) {
    let bridge;
    const f = setup({ beforeBinding(nodes) { nodes[kind].addEventListener(kind === "input" ? "input" : "click", () => bridge.revoke()); } });
    bridge = f.bridge;
    try {
      f.application.attach(f.bridge);
      const reply = kind === "input"
        ? await f.call("gui.set_text", { ...await f.target("input"), text: "retained DOM" })
        : await f.call("gui.click", await f.target("save"));
      assert.notEqual(reply.status, "ok");
      assert.deepEqual(f.state, { draft: "", saved: "", clicks: 0 });
      assert.equal(f.counts()[kind === "input" ? "inputs" : "clicks"], 0);
      if (kind === "input") assert.equal(f.input.value, "retained DOM");
    } finally { f.close(); }
  }
});

test("beforeCommit cannot reenter, dispose or revoke and still commit a mutation", async () => {
  for (const mode of ["reenter", "dispose", "revoke"]) {
    let f;
    f = setup({ beforeCommit() {
      if (mode === "reenter") f.contexts[0].commit(() => { throw new Error("nested mutation entered"); });
      if (mode === "dispose") f.save.dispose();
      if (mode === "revoke") f.bridge.revoke();
    } });
    try {
      f.application.attach(f.bridge);
      assert.notEqual((await f.call("gui.click", await f.target("save"))).status, "ok", mode);
      assert.equal(f.state.clicks, 0, mode);
    } finally { f.close(); }
  }
});

test("settled agent and manual contexts cannot perform additional commits", async () => {
  const f = setup();
  try {
    f.clickManually(); await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
    assert.throws(() => f.contexts[0].commit(() => f.state.clicks++), /dom_action_finished/);
    f.application.attach(f.bridge);
    assert.equal((await f.call("gui.click", await f.target("save"))).status, "ok");
    assert.throws(() => f.contexts[1].commit(() => f.state.clicks++), /dom_action_finished/);
    assert.equal(f.state.clicks, 2);
  } finally { f.close(); }
});

test("async input handlers, synchronous hooks and commit callbacks are refused without unhandled rejection", async () => {
  const rejected = () => Promise.reject(new Error("async hook rejected"));
  const nodes = new Map();
  const document = { getElementById: (id) => nodes.get(id), defaultView: { Event } };
  for (const mode of ["input", "validate", "before", "commit", "thenable"]) {
    const input = new Element("input", "TEXTAREA", document), button = new Element("button", "BUTTON", document);
    nodes.set("input", input); nodes.set("button", button);
    let mutations = 0, getterCalls = 0;
    const errors = [];
    const onError = (error) => { errors.push(error); return rejected(); };
    const text = createDomTextControl({ element: input, read() { throw new Error("not read"); }, changed() {}, onError,
      validateText: mode === "validate" ? rejected : undefined,
      onInput: mode === "input" ? rejected : () => { mutations++; },
    });
    const save = createDomButtonControl({ element: button, read() { throw new Error("not read"); }, changed() {}, onError,
      beforeCommit: mode === "before" ? rejected : undefined,
      onClick(context) {
        context.commit(() => {
          if (mode === "commit") return rejected();
          if (mode === "thenable") return { get then() { getterCalls++; return () => {}; } };
          mutations++;
        });
      },
    });
    try {
      (mode === "input" || mode === "validate" ? input : button).dispatchEvent(new Event(mode === "input" || mode === "validate" ? "input" : "click"));
      await new Promise((resolve) => setImmediate(resolve));
      assert.equal(mutations, 0, mode); assert.equal(getterCalls, 0, mode);
      assert.equal(errors.length, 1, mode);
    } finally { text.dispose(); save.dispose(); }
  }
});

test("a reader cannot replace or dispose its element and publish stale observations", () => {
  for (const mode of ["replace", "dispose"]) {
    const nodes = new Map();
    const document = { getElementById: (id) => nodes.get(id), defaultView: { Event } };
    const element = new Element("button", "BUTTON", document); nodes.set("button", element);
    let binding;
    binding = createDomButtonControl({ element, changed() {}, onClick() {}, read() {
      if (mode === "replace") nodes.set("button", new Element("button", "BUTTON", document));
      else binding.dispose();
      return { role: "button", title: "Old", value: "", enabled: true, editable: false, clicks: 0 };
    } });
    try { assert.throws(() => binding.control.read(), /replaced|disposed/); }
    finally { binding.dispose(); }
  }
});

test("text validation cannot dispose or cancel and then assign DOM or enter onInput", async () => {
  for (const origin of ["agent", "manual"]) for (const mode of ["dispose", "cancel"]) {
    const nodes = new Map();
    const document = { getElementById: (id) => nodes.get(id), defaultView: { Event } };
    const element = new Element("input", "TEXTAREA", document); nodes.set("input", element);
    let inputs = 0, binding;
    const controller = new AbortController();
    binding = createDomTextControl({ element, changed() {},
      read: () => ({ role: "text_field", title: "Input", value: element.value, enabled: true, editable: true, clicks: 0 }),
      validateText() { if (mode === "dispose") binding.dispose(); else controller.abort(); },
      onInput() { inputs++; },
    });
    try {
      if (origin === "agent") {
        await assert.rejects(binding.control.setText("new", { signal: controller.signal, commit: (mutate) => mutate(), changed() {} }));
        assert.equal(element.value, "");
      } else {
        // A manual binding's signal is owned by disposal. Cancellation of an
        // unrelated controller must not cancel independently authorized input.
        element.value = "manual";
        element.dispatchEvent(new Event("input"));
      }
      assert.equal(inputs, origin === "manual" && mode === "cancel" ? 1 : 0);
    } finally { binding.dispose(); }
  }
});
