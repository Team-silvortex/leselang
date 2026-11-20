import test from "node:test";
import assert from "node:assert/strict";
import { createArticleView } from "../dist/article/view.js";
import { Bridge } from "../dist/bridge.js";

class Element extends EventTarget {
  constructor(id) {
    super(); this.id = id;
    this.tagName = ["title", "content"].includes(id) ? "TEXTAREA" : ["publish", "other"].includes(id) ? "BUTTON" : "SPAN";
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

function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));

function connection(session = "article-task") {
  const scope = { window: "main", epoch: "article-page", session };
  const bridge = new Bridge(scope);
  let request = 0;
  return { bridge, call: (operation, arguments_ = {}) => bridge.handle({ transport_version: 1,
    ...scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ }) };
}

async function withArticle(run, options = {}) {
  const previous = globalThis.document, nodes = new Map();
  const ids = { title: "title", content: "content", titleLabel: "title-label", contentLabel: "content-label",
    publish: "publish", other: "other", savedTitle: "saved-title", savedContent: "saved-content",
    publishCount: "publish-count", otherCount: "other-count", status: "status" };
  const elements = Object.fromEntries(Object.entries(ids).map(([key, id]) => {
    const node = new Element(id); nodes.set(id, node); return [key, node];
  }));
  elements.titleLabel.textContent = "Article title";
  elements.contentLabel.textContent = "Article content";
  elements.publish.textContent = "Publish article";
  elements.other.textContent = "Unrelated action";
  globalThis.document = { defaultView: { Event }, getElementById(id) { return nodes.get(id); } };
  for (const element of nodes.values()) element.ownerDocument = globalThis.document;
  const view = createArticleView(elements, options), { bridge, call } = connection();
  const observed = (identifier) => view.observe().find((row) => row.identifier === identifier);
  const input = (field, text) => { elements[field].value = text; elements[field].dispatchEvent(new Event("input")); };
  const freshEditor = () => {
    const next = {};
    for (const key of ["title", "content", "titleLabel", "contentLabel", "publish", "other"]) {
      const old = elements[key];
      old.isConnected = false;
      const node = new Element(old.id);
      node.ownerDocument = globalThis.document;
      node.textContent = old.textContent;
      nodes.set(node.id, node);
      elements[key] = node;
      next[key] = node;
    }
    return next;
  };
  try { await run({ view, elements, nodes, bridge, call, observed, input, freshEditor }); }
  finally { view.detach(bridge); view.dispose(); globalThis.document = previous; }
}

async function target(call, name) {
  const target = (await call("gui.find", { name })).value;
  const observed = (await call("gui.observe", { target })).value;
  return { target, expected_revision: observed.revision };
}
async function setText(call, field, text) {
  return call("gui.set_text", { ...await target(call, field), text });
}

test("manual and registered article controls publish independent title and content through real handlers", async () => {
  await withArticle(async ({ view, elements, bridge, call, observed, input }) => {
    input("title", "manual title"); input("content", "manual content");
    elements.other.click(); elements.publish.click();
    await tick();
    assert.equal(observed("publish").title, "manual title");
    assert.equal(observed("publish").value, "manual content");
    assert.equal(observed("publish").clicks, 1);
    const unrelated = observed("other");
    view.attach(bridge);
    assert.deepEqual((await call("gui.list")).value, ["title", "content", "publish", "other"]);
    const title = '标题 "quoted" ✓', content = '正文\\path\n{ "task": "other" }';
    assert.equal((await setText(call, "title", title)).status, "ok");
    assert.equal((await setText(call, "content", content)).status, "ok");
    assert.equal((await call("gui.click", await target(call, "publish"))).status, "ok");
    assert.equal(elements.title.value, title); assert.equal(elements.content.value, content);
    assert.equal(elements.savedTitle.textContent, title); assert.equal(elements.savedContent.textContent, content);
    assert.equal(elements.publishCount.textContent, "2");
    assert.equal(observed("publish").title, title); assert.equal(observed("publish").value, content);
    assert.deepEqual(observed("other"), unrelated, "unrelated state is preserved, even when its count was already nonzero");
    view.detach(bridge);
    input("title", "manual after detach");
    elements.publish.click(); await tick();
    assert.equal(observed("publish").title, "manual after detach");
    assert.equal(observed("publish").clicks, 3);
  });
});

for (const field of ["title", "content"]) test(`manual ${field} ABA invalidates an observed publication revision`, async () => {
  await withArticle(async ({ view, bridge, call, observed, input }) => {
    input(field, "original"); view.attach(bridge);
    const action = await target(call, "publish");
    input(field, "temporary"); input(field, "original");
    const reply = await call("gui.click", action);
    assert.equal(reply.status, "rejected"); assert.equal(reply.error, "stale_revision");
    assert.equal(observed("publish").clicks, 0);
  });
});

test("an invalid second field preserves the already changed title without publication", async () => {
  await withArticle(async ({ view, bridge, call, observed, input, elements }) => {
    input("content", "original content"); view.attach(bridge);
    assert.equal((await setText(call, "title", "retained title")).status, "ok");
    assert.equal((await setText(call, "content", "\0")).status, "uncertain", "application-level validation follows adapter invocation");
    assert.equal(observed("title").value, "retained title");
    assert.equal(observed("content").value, "original content");
    assert.equal(observed("publish").clicks, 0);
    input("content", "文".repeat(1366));
    assert.equal(elements.content.value, "original content", "invalid manual input restores the valid draft");
    assert.equal(observed("content").value, "original content");
  });
});

for (const stop of ["cancel", "revoke"]) test(`${stop} blocks late publication while retaining both prior field edits`, async () => {
  const entered = deferred(), release = deferred();
  await withArticle(async ({ view, bridge, call, observed }) => {
    view.attach(bridge);
    await setText(call, "title", "retained title"); await setText(call, "content", "retained content");
    const pending = call("gui.click", await target(call, "publish"));
    await entered.promise;
    if (stop === "cancel") bridge.cancel("article-task"); else bridge.revoke();
    assert.notEqual((await pending).status, "ok");
    release.resolve(); await tick();
    assert.equal(observed("title").value, "retained title");
    assert.equal(observed("content").value, "retained content");
    assert.equal(observed("publish").clicks, 0);
    assert.equal(observed("publish").value, "");
    assert.equal(observed("other").clicks, 0);
  }, { async beforePublish() { entered.resolve(); await release.promise; } });
});

test("a detached pending publication cannot commit or invalidate a replacement session", async () => {
  const entered = deferred(), release = deferred();
  await withArticle(async ({ view, bridge, call, observed, input }) => {
    input("title", "retained title"); input("content", "retained content"); view.attach(bridge);
    const pending = call("gui.click", await target(call, "publish"));
    await entered.promise;
    view.detach(bridge);
    const next = connection("replacement-task");
    try {
      view.attach(next.bridge);
      const action = await target(next.call, "publish");
      assert.notEqual((await pending).status, "ok");
      release.resolve(); await tick();
      assert.equal(observed("publish").clicks, 0);
      assert.equal((await next.call("gui.observe", { target: action.target })).value.revision, action.expected_revision);
      view.detach(bridge);
      assert.equal((await next.call("gui.click", action)).status, "ok");
      assert.equal(observed("publish").title, "retained title");
      assert.equal(observed("publish").value, "retained content");
      assert.equal(observed("publish").clicks, 1);
    } finally { view.detach(next.bridge); }
  }, { async beforePublish() { entered.resolve(); await release.promise; } });
});

test("DOM edits and replaced nodes after publication begins block its late domain mutation", async () => {
  for (const change of [
    ({ elements }) => { elements.title.value = "unreported title"; },
    ({ elements }) => { elements.content.value = "unreported content"; },
    ({ elements }) => { elements.savedTitle.textContent = "forged saved title"; },
    ({ elements }) => { elements.publishCount.textContent = "9"; },
    ({ elements, nodes }) => { elements.other.isConnected = false; nodes.set("other", new Element("other")); },
  ]) await withArticle(async (fixture) => {
    const { view, elements, bridge, call, input } = fixture;
    input("title", "draft title"); input("content", "draft content"); view.attach(bridge);
    elements.publish.afterClick = () => change(fixture);
    assert.notEqual((await call("gui.click", await target(call, "publish"))).status, "ok");
    await tick();
    assert.equal(elements.savedContent.textContent, "");
    assert.throws(() => view.observe(), /DOM instance has been replaced|DOM and application state differ/u);
  });
});

test("observation verifies both saved fields, both counters, labels and real DOM instances", async () => {
  for (const key of ["savedTitle", "savedContent", "publishCount", "otherCount", "titleLabel", "contentLabel", "publish", "other"]) {
    await withArticle(async ({ view, elements, nodes }) => {
      const original = elements[key].textContent;
      elements[key].textContent = "forged";
      assert.throws(() => view.observe(), /DOM and application state differ/u, key);
      elements[key].textContent = original;
      nodes.set(elements[key].id, new Element(elements[key].id));
      assert.throws(() => view.observe(), /DOM instance has been replaced/u, key);
    });
  }
});

test("partial article attachment closes the bridge and permits a later recovered session", async () => {
  await withArticle(async ({ view, elements, nodes, bridge, call }) => {
    nodes.set("publish", new Element("publish"));
    assert.throws(() => view.attach(bridge), /DOM instance has been replaced/u);
    assert.equal((await call("gui.list")).status, "rejected");
    nodes.set("publish", elements.publish);
    const next = connection("recovered-task");
    try {
      view.attach(next.bridge);
      assert.deepEqual((await next.call("gui.list")).value, ["title", "content", "publish", "other"]);
    } finally { view.detach(next.bridge); }
  });
});

test("a failure after publication keeps real saved evidence without a hidden retry", async () => {
  await withArticle(async ({ view, elements, bridge, call, observed, input }) => {
    input("title", "committed title"); input("content", "committed content"); view.attach(bridge);
    let status = elements.status.textContent;
    Object.defineProperty(elements.status, "textContent", {
      get() { return status; },
      set(value) { if (value.startsWith("已发布。")) throw new Error("status rendering failed"); status = value; },
    });
    assert.equal((await call("gui.click", await target(call, "publish"))).status, "uncertain");
    await tick();
    assert.equal(observed("publish").title, "committed title");
    assert.equal(observed("publish").value, "committed content");
    assert.equal(observed("publish").clicks, 1);
    assert.equal(observed("other").clicks, 0);
  });
});

test("preview unmounts the actual controls and remounts new instances without changing article state", async () => {
  await withArticle(async ({ view, elements, bridge, call, observed, input, freshEditor }) => {
    input("title", "retained title"); input("content", "retained content");
    elements.publish.click(); await tick();
    const before = view.observe(), oldPublish = elements.publish, oldTitle = elements.title;
    const draft = view.draft(), initialBinding = view.bindingRevision();
    view.attach(bridge);
    const previous = await target(call, "publish");
    view.unmountEditor();
    assert.equal(view.bindingRevision(), initialBinding + 4);
    assert.deepEqual(view.observe(), []);
    assert.deepEqual((await call("gui.list")).value, []);
    assert.equal((await call("gui.click", previous)).status, "rejected");
    assert.deepEqual(view.draft(), draft);
    oldPublish.click();
    oldTitle.value = "detached edit"; oldTitle.dispatchEvent(new Event("input"));
    await tick();
    assert.deepEqual(view.draft(), draft, "disposed listeners cannot change the retained draft");
    view.mountEditor(freshEditor());
    assert.equal(view.bindingRevision(), initialBinding + 8);
    assert.notEqual(elements.publish, oldPublish);
    assert.deepEqual(view.observe(), before, "same values do not imply the same binding identity");
    assert.equal(elements.title.value, draft.title);
    assert.equal(elements.content.value, draft.content);
    assert.equal((await call("gui.click", previous)).status, "rejected");
    const current = await target(call, "publish");
    assert.notEqual(current.target.generation, previous.target.generation);
    assert.equal((await call("gui.click", current)).status, "ok");
    assert.equal(observed("publish").clicks, 2);
    assert.equal(observed("other").clicks, 0);
  });
});

for (const origin of ["agent", "manual"]) test(`preview fences an awaited ${origin} publication from a later editor`, async () => {
  const entered = deferred(), release = deferred();
  await withArticle(async ({ view, elements, bridge, call, observed, input, freshEditor }) => {
    input("title", "pending title"); input("content", "pending content");
    view.attach(bridge);
    let pending;
    if (origin === "agent") pending = call("gui.click", await target(call, "publish"));
    else elements.publish.click();
    await entered.promise;
    const oldButton = elements.publish;
    view.unmountEditor();
    view.detach(bridge);
    view.mountEditor(freshEditor());
    const next = connection("continued-editor");
    try {
      view.attach(next.bridge);
      const after = await target(next.call, "other");
      release.resolve(); await tick(); await tick();
      if (pending) assert.notEqual((await pending).status, "ok");
      assert.equal(observed("publish").clicks, 0);
      assert.equal(observed("publish").value, "");
      assert.equal(observed("title").value, "pending title");
      assert.equal(observed("content").value, "pending content");
      assert.equal((await next.call("gui.observe", { target: after.target })).value.revision, after.expected_revision);
      oldButton.click(); await tick();
      assert.equal(observed("publish").clicks, 0);
      assert.equal(elements.status.dataset.failed, "false", "an old rejection cannot replace the new editor's status");
      assert.equal((await next.call("gui.click", await target(next.call, "publish"))).status, "ok");
      assert.equal(observed("publish").clicks, 1);
    } finally { view.detach(next.bridge); }
  }, { async beforePublish() { entered.resolve(); await release.promise; } });
});

test("failed editor creation disposes its partial DOM bindings and preserves publication evidence", async () => {
  await withArticle(async ({ view, elements, input, freshEditor }) => {
    input("title", "draft"); input("content", "body");
    elements.publish.click(); await tick();
    view.unmountEditor();
    const editor = freshEditor();
    editor.publish.type = "submit";
    assert.throws(() => view.mountEditor(editor), /invalid_dom_control_element/u);
    assert.deepEqual(view.observe(), []);
    assert.deepEqual(view.draft(), { title: "draft", content: "body" });
    assert.equal(elements.savedTitle.textContent, "draft");
    assert.equal(elements.publishCount.textContent, "1");
    editor.publish.type = "button";
    view.mountEditor(editor);
    assert.equal(view.observe().find((row) => row.identifier === "publish").clicks, 1);
  });
});

test("disposing a previewed view is idempotent and cannot recreate controls", async () => {
  await withArticle(async ({ view, freshEditor }) => {
    view.unmountEditor();
    view.dispose(); view.dispose();
    assert.deepEqual(view.observe(), []);
    assert.throws(() => view.mountEditor(freshEditor()), /disposed/u);
    assert.throws(() => view.draft(), /disposed/u);
  });
});
