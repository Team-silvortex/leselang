import test from "node:test";
import assert from "node:assert/strict";
import { ArticleModel } from "../dist/article/model.js";

const direct = { signal: new AbortController().signal, commit(mutation) { return mutation(); } };

test("article publication crosses a Promise and publishes two independent fields once", async () => {
  const model = new ArticleModel();
  const title = '标题 "quoted" ✓', content = '正文\\path\n{ "task": "other" }';
  model.input("title", title);
  model.input("content", content);
  model.other();
  const before = model.snapshot();
  let renders = 0;
  const pending = model.publish(direct, () => { renders++; });
  assert.deepEqual(model.snapshot(), before);
  assert.equal(renders, 0);
  await pending;
  assert.deepEqual(model.snapshot(), { title, content, savedTitle: title, savedContent: content, publishes: 1, otherClicks: 1 });
  assert.equal(renders, 1);
  before.title = "owned snapshot";
  assert.equal(model.snapshot().title, title);
});

test("article fields reject invalid text before altering a prior valid draft", () => {
  const model = new ArticleModel();
  model.input("title", "retained title");
  model.input("content", "retained content");
  const before = model.snapshot();
  for (const field of ["title", "content"]) {
    for (const text of [null, 42, "\0", "\ud800", "\udc00", "x".repeat(4097), "文".repeat(1366)]) {
      assert.throws(() => model.input(field, text), /invalid article text/u);
      assert.deepEqual(model.snapshot(), before);
    }
  }
  for (const field of ["other", "__proto__", null]) assert.throws(() => model.input(field, "x"), /unknown article field/u);
  model.input("content", "x".repeat(4096));
  assert.equal(model.snapshot().content.length, 4096);
});

for (const field of ["title", "content"]) test(`${field} ABA edits prevent a pending publication`, async () => {
  const model = new ArticleModel();
  model.input("title", "title");
  model.input("content", "content");
  const pending = model.publish(direct, () => assert.fail("stale publication cannot render"));
  model.input(field, "temporary");
  model.input(field, field);
  await assert.rejects(pending, /article changed before publication/u);
  assert.equal(model.snapshot().publishes, 0);
  assert.equal(model.snapshot().savedTitle, "");
  assert.equal(model.snapshot().savedContent, "");
});

test("cancellation retains both drafts and fences publication without retry", async () => {
  const model = new ArticleModel();
  model.input("title", "retained title");
  model.input("content", "retained content");
  const controller = new AbortController();
  let attempts = 0;
  const pending = model.publish({ signal: controller.signal, commit(mutation) {
    attempts++;
    if (controller.signal.aborted) throw new Error("cancelled");
    return mutation();
  } }, () => assert.fail("cancelled publication cannot render"));
  controller.abort();
  await assert.rejects(pending, /cancelled/u);
  assert.equal(attempts, 1);
  assert.deepEqual(model.snapshot(), { title: "retained title", content: "retained content", savedTitle: "", savedContent: "", publishes: 0, otherClicks: 0 });
});

test("concurrent publication is stale, while a separate later publication remains available", async () => {
  const model = new ArticleModel();
  model.input("title", "title");
  model.input("content", "first content");
  let renders = 0;
  const first = model.publish(direct, () => { renders++; });
  const second = model.publish(direct, () => assert.fail("concurrent publication must not render"));
  await first;
  await assert.rejects(second, /article changed before publication/u);
  model.input("content", "second content");
  await model.publish(direct, () => { renders++; });
  assert.equal(renders, 2);
  assert.equal(model.snapshot().publishes, 2);
  assert.equal(model.snapshot().savedTitle, "title");
  assert.equal(model.snapshot().savedContent, "second content");
});

test("failure after domain commit preserves both published fields and the count", async () => {
  const model = new ArticleModel();
  model.input("title", "committed title");
  model.input("content", "committed content");
  await assert.rejects(model.publish(direct, () => { throw new Error("render failed"); }), /render failed/u);
  assert.deepEqual(model.snapshot(), { title: "committed title", content: "committed content", savedTitle: "committed title", savedContent: "committed content", publishes: 1, otherClicks: 0 });
});
