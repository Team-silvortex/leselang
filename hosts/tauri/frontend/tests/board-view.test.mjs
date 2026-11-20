import test from "node:test";
import assert from "node:assert/strict";
import { createBoardView } from "../dist/board/view.js";
import { Bridge } from "../dist/bridge.js";

class Element extends EventTarget {
  constructor(id) { super(); this.id = id; this.tagName = id.endsWith("-complete") ? "BUTTON" : "SPAN"; }
  type = "button";
  isConnected = true;
  disabled = false;
  textContent = "";
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

async function withBoard(run) {
  const previous = globalThis.document;
  const nodes = new Map();
  const node = (id) => { const value = new Element(id); nodes.set(id, value); return value; };
  const task = (id) => ({ title: node(`${id}-title`), button: node(`${id}-complete`),
    status: node(`${id}-state`), completions: node(`${id}-count`) });
  const elements = { review: task("review"), archive: task("archive"), status: node("status") };
  globalThis.document = { defaultView: { Event }, getElementById(id) { return nodes.get(id); } };
  for (const element of nodes.values()) element.ownerDocument = globalThis.document;
  const view = createBoardView(elements);
  const scope = { window: "main", epoch: "board-page", session: "board-task" };
  const bridge = new Bridge(scope);
  let request = 0;
  const call = (operation, arguments_ = {}) => bridge.handle({ transport_version: 1,
    ...scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ });
  try { await run({ view, elements, nodes, bridge, call }); }
  finally { bridge.close(); view.dispose(); globalThis.document = previous; }
}

test("manual board buttons and bridge actions share the real completion handler", async () => {
  await withBoard(async ({ view, elements, bridge, call }) => {
    elements.archive.button.click();
    assert.equal(view.observe()[1].value, "pending");
    await Promise.resolve();
    assert.deepEqual(view.observe()[1], { identifier: "archive", role: "button", title: "整理示例资料",
      value: "done", enabled: false, editable: false, clicks: 1 });
    view.attach(bridge);
    assert.deepEqual((await call("gui.list")).value, ["review", "archive"]);
    const target = (await call("gui.find", { name: "review" })).value;
    const before = (await call("gui.observe", { target })).value;
    const reply = await call("gui.click", { target, expected_revision: before.revision });
    assert.equal(reply.status, "ok");
    const after = (await call("gui.observe", { target })).value;
    assert.equal(after.value, "done");
    assert.equal(after.enabled, false);
    assert.equal(after.clicks, 1);
    assert.equal(elements.review.button.disabled, true);
    assert.equal(elements.review.status.textContent, "done");
    assert.equal(elements.review.completions.textContent, "1");
    assert.equal(view.observe()[1].clicks, 1, "the earlier manual completion is unchanged");
    const repeated = await call("gui.click", { target, expected_revision: after.revision });
    assert.equal(repeated.status, "rejected");
    assert.equal(repeated.error, "disabled");
    elements.review.button.click();
    await Promise.resolve();
    assert.equal(view.observe()[0].clicks, 1);
    view.detach(bridge);
  });
});

test("detaching after the DOM handler starts fences its later Promise commit", async () => {
  await withBoard(async ({ view, elements, bridge, call }) => {
    view.attach(bridge);
    const target = (await call("gui.find", { name: "review" })).value;
    const before = (await call("gui.observe", { target })).value;
    elements.review.button.afterClick = () => view.detach(bridge);
    const reply = await call("gui.click", { target, expected_revision: before.revision });
    assert.notEqual(reply.status, "ok");
    await Promise.resolve();
    assert.equal(view.observe()[0].value, "pending");
    assert.equal(view.observe()[0].clicks, 0);
    const next = new Bridge({ window: "main", epoch: "board-page", session: "next-task" });
    view.attach(next);
    view.detach(next);
  });
});

test("a replaced control after click cannot commit through its stale DOM instance", async () => {
  await withBoard(async ({ view, elements, nodes, bridge, call }) => {
    view.attach(bridge);
    const target = (await call("gui.find", { name: "review" })).value;
    const before = (await call("gui.observe", { target })).value;
    const button = elements.review.button;
    button.afterClick = () => {
      button.isConnected = false;
      nodes.set(button.id, new Element(button.id));
    };
    const reply = await call("gui.click", { target, expected_revision: before.revision });
    assert.notEqual(reply.status, "ok");
    await Promise.resolve();
    assert.equal(elements.review.status.textContent, "pending");
    assert.equal(elements.review.completions.textContent, "0");
    assert.throws(() => view.observe(), /DOM instance has been replaced/u);
  });
});

test("application observations reject altered DOM fields instead of accepting forged board state", async () => {
  for (const mutate of [
    (e) => { e.review.status.textContent = "done"; },
    (e) => { e.review.completions.textContent = "1"; },
    (e) => { e.review.title.textContent = "replaced title"; },
    (e) => { e.review.button.disabled = true; },
    (e) => { e.review.button.textContent = "different action"; },
  ]) await withBoard(async ({ view, elements }) => {
    mutate(elements);
    assert.throws(() => view.observe(), /DOM and application state differ/u);
    elements.archive.button.click();
    await Promise.resolve();
    await Promise.resolve();
    assert.equal(elements.archive.status.textContent, "pending", "tampered observations prevent pending mutation");
    assert.equal(elements.archive.completions.textContent, "0");
  });
});

test("a second bridge cannot attach while the original workflow owns the board", async () => {
  await withBoard(async ({ view, bridge }) => {
    view.attach(bridge);
    const other = new Bridge({ window: "main", epoch: "other", session: "other" });
    assert.throws(() => view.attach(other), /already[_ ]attached/u);
    view.detach(bridge);
    view.attach(other);
    view.detach(other);
  });
});

test("an old board completion and detach cannot affect a replacement workflow", async () => {
  await withBoard(async ({ view, elements, bridge, call }) => {
    view.attach(bridge);
    const target = (await call("gui.find", { name: "review" })).value;
    const before = (await call("gui.observe", { target })).value;
    const scope = { window: "main", epoch: "board-page", session: "replacement" };
    const next = new Bridge(scope);
    let request = 0;
    const nextCall = (operation, arguments_ = {}) => next.handle({ transport_version: 1,
      ...scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ });
    elements.review.button.afterClick = () => {
      view.detach(bridge);
      view.attach(next);
    };
    try {
      const reply = await call("gui.click", { target, expected_revision: before.revision });
      assert.notEqual(reply.status, "ok");
      assert.equal(view.observe()[0].value, "pending");
      assert.equal(view.observe()[0].clicks, 0);
      view.detach(bridge);
      const nextTarget = (await nextCall("gui.find", { name: "archive" })).value;
      const nextBefore = (await nextCall("gui.observe", { target: nextTarget })).value;
      assert.equal((await nextCall("gui.click", { target: nextTarget, expected_revision: nextBefore.revision })).status, "ok");
      assert.equal(view.observe()[0].clicks, 0);
      assert.equal(view.observe()[1].clicks, 1);
    } finally { view.detach(next); }
  });
});
