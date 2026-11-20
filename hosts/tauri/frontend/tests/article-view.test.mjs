import test from "node:test";
import assert from "node:assert/strict";
import { createArticleView } from "../dist/article/view.js";
import { Bridge } from "../dist/bridge.js";

class Element extends EventTarget {
  constructor(id) { super(); this.id = id; }
  isConnected = true;
  disabled = false;
  readOnly = false;
  textContent = "";
  value = "";
  dataset = {};
  afterClick;
  click() {
    if (this.disabled) return;
    this.dispatchEvent(new Event("click"));
    this.afterClick?.();
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
  globalThis.document = { getElementById(id) { return nodes.get(id); } };
  const view = createArticleView(elements, options), { bridge, call } = connection();
  const observed = (identifier) => view.observe().find((row) => row.identifier === identifier);
  const input = (field, text) => { elements[field].value = text; elements[field].dispatchEvent(new Event("input")); };
  try { await run({ view, elements, nodes, bridge, call, observed, input }); }
  finally { view.detach(bridge); globalThis.document = previous; }
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
