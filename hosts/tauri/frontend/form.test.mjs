import test from "node:test";
import assert from "node:assert/strict";
import { FormModel } from "./dist/form.js";
import { Bridge } from "./dist/bridge.js";

const direct = {
  signal: new AbortController().signal,
  commit(mutation) { return mutation(); },
};

test("the real save model commits only after its Promise boundary", async () => {
  const model = new FormModel();
  const text = '保存 "✓"\\\n';
  model.input(text);
  let publications = 0;
  const pending = model.save(direct, () => { publications++; });
  assert.deepEqual(model.snapshot(), { draft: text, saved: "", saveClicks: 0, otherClicks: 0 });
  assert.equal(publications, 0);
  await pending;
  assert.deepEqual(model.snapshot(), { draft: text, saved: text, saveClicks: 1, otherClicks: 0 });
  assert.equal(publications, 1);
});

test("cancellation before commit prevents save and has no implicit retry", async () => {
  const model = new FormModel();
  model.input("one");
  const controller = new AbortController();
  let attempts = 0;
  const pending = model.save({
    signal: controller.signal,
    commit(mutation) {
      attempts++;
      if (controller.signal.aborted) throw new Error("cancelled");
      return mutation();
    },
  }, () => assert.fail("cancelled save cannot publish"));
  controller.abort();
  await assert.rejects(pending, /cancelled/);
  assert.equal(attempts, 1);
  assert.equal(model.snapshot().saved, "");
  assert.equal(model.snapshot().saveClicks, 0);
});

test("input ABA is detected across an asynchronous save", async () => {
  const model = new FormModel();
  model.input("A");
  const pending = model.save(direct, () => assert.fail("stale save cannot publish"));
  model.input("B");
  model.input("A");
  await assert.rejects(pending, /input changed/);
  assert.equal(model.snapshot().saved, "");
  assert.equal(model.snapshot().saveClicks, 0);
});

test("a failure after model mutation retains the partial effect", async () => {
  const model = new FormModel();
  model.input("saved before failure");
  await assert.rejects(model.save(direct, () => { throw new Error("render failed"); }), /render failed/);
  assert.equal(model.snapshot().saved, "saved before failure");
  assert.equal(model.snapshot().saveClicks, 1);
  assert.equal(model.snapshot().otherClicks, 0);
});

test("registered model adapters preserve dependency revisions through the bridge", async () => {
  const model = new FormModel();
  const scope = { window: "main", epoch: "page-one", session: "session-one" };
  const bridge = new Bridge(scope);
  bridge.register("input", {
    read() {
      return { role: "text_field", title: "Text to save", value: model.snapshot().draft,
        enabled: true, editable: true, clicks: 0 };
    },
    setText(text, context) {
      context.commit(() => {
        model.input(text);
        bridge.changed("input");
        bridge.changed("save");
      });
    },
  });
  bridge.register("save", {
    read() {
      const state = model.snapshot();
      return { role: "button", title: "Save", value: state.saved,
        enabled: true, editable: false, clicks: state.saveClicks };
    },
    click(context) { return model.save(context, () => bridge.changed("save")); },
  });
  let id = 0;
  const call = async (operation, arguments_) => bridge.handle({
    transport_version: 1, ...scope, request_id: ++id, budget_ms: 1000,
    operation, arguments: arguments_,
  });
  const input = (await call("gui.find", { name: "input" })).value;
  const save = (await call("gui.find", { name: "save" })).value;
  const beforeInput = (await call("gui.observe", { target: input })).value;
  const oldSave = (await call("gui.observe", { target: save })).value;
  const typed = await call("gui.set_text", {
    target: input, expected_revision: beforeInput.revision, text: "real TS model",
  });
  assert.equal(typed.status, "ok");
  const stale = await call("gui.click", { target: save, expected_revision: oldSave.revision });
  assert.equal(stale.status, "rejected");
  assert.equal(stale.error, "stale_revision");
  assert.equal(model.snapshot().saveClicks, 0);
  const beforeSave = (await call("gui.observe", { target: save })).value;
  const clicked = await call("gui.click", { target: save, expected_revision: beforeSave.revision });
  assert.equal(clicked.status, "ok");
  const afterSave = (await call("gui.observe", { target: save })).value;
  assert.equal(afterSave.value, "real TS model");
  assert.equal(afterSave.clicks, 1);
});
