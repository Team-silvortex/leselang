import test from "node:test";
import assert from "node:assert/strict";
import { BoardModel } from "../dist/board/model.js";

const direct = { signal: new AbortController().signal, commit(mutation) { return mutation(); } };

test("board completion crosses a Promise before changing one task and publishing", async () => {
  const model = new BoardModel();
  const before = model.snapshot();
  let publications = 0;
  const pending = model.complete("review", direct, () => { publications++; });
  assert.deepEqual(model.snapshot(), before);
  assert.equal(publications, 0);
  await pending;
  assert.deepEqual(model.task("review"), { id: "review", title: "核对示例报告", status: "done", completions: 1 });
  assert.deepEqual(model.task("archive"), before[1]);
  assert.equal(publications, 1);
  assert.deepEqual(before[0], { id: "review", title: "核对示例报告", status: "pending", completions: 0 });
});

test("either task can complete without special agent logic and snapshots are owned", async () => {
  const model = new BoardModel();
  const snapshot = model.snapshot();
  assert.ok(Object.isFrozen(snapshot[0]));
  assert.throws(() => { snapshot[0].status = "done"; }, TypeError);
  snapshot.pop();
  assert.equal(model.snapshot().length, 2);
  await model.complete("archive", direct, () => {});
  assert.equal(model.task("archive").status, "done");
  assert.equal(model.task("review").status, "pending");
  await model.complete("review", direct, () => {});
  assert.ok(model.snapshot().every((task) => task.status === "done" && task.completions === 1));
});

test("repeated and concurrent completion cannot increment a completed task again", async () => {
  const model = new BoardModel();
  let publications = 0;
  const first = model.complete("review", direct, () => { publications++; });
  const second = model.complete("review", direct, () => { publications++; });
  await first;
  await assert.rejects(second, /changed before completion/u);
  await assert.rejects(model.complete("review", direct, () => assert.fail()), /already completed/u);
  assert.equal(publications, 1);
  assert.equal(model.task("review").completions, 1);
  assert.equal(model.task("archive").completions, 0);
});

test("a retired action context prevents a pending completion with no retry", async () => {
  const model = new BoardModel();
  const controller = new AbortController();
  let commits = 0;
  const pending = model.complete("review", {
    signal: controller.signal,
    commit(mutation) {
      commits++;
      if (controller.signal.aborted) throw new Error("cancelled");
      return mutation();
    },
  }, () => assert.fail("cancelled completion must not publish"));
  controller.abort();
  await assert.rejects(pending, /cancelled/u);
  assert.equal(commits, 1);
  assert.equal(model.task("review").status, "pending");
  assert.equal(model.task("review").completions, 0);
});

test("failure while publishing retains a committed completion without rollback", async () => {
  const model = new BoardModel();
  await assert.rejects(model.complete("archive", direct, () => { throw new Error("render failed"); }), /render failed/u);
  assert.equal(model.task("archive").status, "done");
  assert.equal(model.task("archive").completions, 1);
  assert.equal(model.task("review").status, "pending");
  await assert.rejects(model.complete("archive", direct, () => assert.fail()), /already completed/u);
});

test("unknown task ids cannot reach a mutation or a publication", async () => {
  const model = new BoardModel();
  for (const id of ["missing", "__proto__", null, 7]) {
    assert.throws(() => model.task(id), /unknown board task/u);
    await assert.rejects(model.complete(id, direct, () => assert.fail()), /unknown board task/u);
  }
  assert.ok(model.snapshot().every((task) => task.completions === 0));
});
