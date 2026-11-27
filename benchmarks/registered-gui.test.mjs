import assert from "node:assert/strict";
import test from "node:test";
import { verifiedObservations } from "../scripts/registered-gui-consumers.mjs";
import { distribution, readTimings } from "./registered-gui-stats.mjs";

test("median includes both middle samples and nearest-rank p95 requires 20 samples", () => {
  assert.deepEqual(distribution([9, 3, 5, 1]), { p50: 4, p95: null });
  const twenty = Array.from({ length: 20 }, (_, i) => 20 - i);
  assert.deepEqual(distribution(twenty.slice(1)), { p50: 10, p95: null });
  assert.deepEqual(distribution(twenty), { p50: 10.5, p95: 19 });
});

const ids = ["publish", "cancel-after-edit"];
const times = { primary_ns: 100, request_span_ns: 70, readback_ns: 50 };
function row(id, overrides = {}) {
  return "LESE_GUI_TIMING " + JSON.stringify({ id, ...times, ...overrides });
}

test("reads ordered timings independently of observations on the same stdout", () => {
  const stdout = [row(ids[0]), 'LESE_GUI {"id":"publish","observed":{}}', row(ids[1])].join("\r\n");
  assert.deepEqual([...readTimings(stdout, ids, "node", 300)], ids.map(id => [id, times]));
});

test("rejects missing, duplicated and reordered timing samples", () => {
  for (const order of [[ids[0]], [ids[0], ids[0]], [...ids].reverse()]) {
    const stdout = order.map(id => row(id)).join("\n");
    assert.throws(() => readTimings(stdout, ids, "node", 400), undefined, JSON.stringify(order));
  }
});

test("rejects invalid nanoseconds, spans outside the primary and excess total wall time", () => {
  for (const invalid of [
    { readback_ns: -1 }, { primary_ns: 100.5 }, { readback_ns: Number.MAX_SAFE_INTEGER + 1 },
    { request_span_ns: 101 },
  ]) {
    const stdout = [row(ids[0], invalid), row(ids[1])].join("\n");
    assert.throws(() => readTimings(stdout, ids, "node", 400), undefined, JSON.stringify(invalid));
  }
  const stdout = ids.map(id => row(id)).join("\n");
  assert.throws(() => readTimings(stdout, ids, "node", 299));
});

test("timing output cannot substitute for the verified application observations", () => {
  const id = ids[0];
  const expected = new Map([[id, { commits: { clicks: 1 } }]]);
  const wrong = { id, observed: { commits: { clicks: 0 } } };
  assert.throws(() => verifiedObservations(row(id), expected, "node"));
  assert.throws(() => verifiedObservations("LESE_GUI " + JSON.stringify(wrong), expected, "node"));
});
