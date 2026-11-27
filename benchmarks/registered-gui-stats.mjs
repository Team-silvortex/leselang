import assert from "node:assert/strict";
import { recordsWithPrefix } from "../scripts/registered-gui-consumers.mjs";

export function distribution(values) {
  assert.ok(values.length > 0, "empty sample");
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return {
    p50: sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2,
    p95: sorted.length < 20 ? null : sorted[Math.ceil(sorted.length * 0.95) - 1],
  };
}

export function readTimings(stdout, ids, name, suiteNs) {
  const records = recordsWithPrefix(stdout, "LESE_GUI_TIMING ", name);
  assert.deepEqual([...records.keys()], ids, `${name}: incomplete or out-of-order timing rows`);
  const timings = new Map();
  let accounted = 0;
  for (const [id, { primary_ns, request_span_ns, readback_ns }] of records) {
    assert.ok([primary_ns, request_span_ns, readback_ns].every(Number.isSafeInteger),
      `${name}/${id}: invalid nanoseconds`);
    assert.ok(primary_ns > 0 && readback_ns > 0 && request_span_ns >= 0 && request_span_ns <= primary_ns,
      `${name}/${id}: invalid timing bounds`);
    accounted += primary_ns + readback_ns;
    timings.set(id, { primary_ns, request_span_ns, readback_ns });
  }
  assert.ok(accounted <= suiteNs, `${name}: case times exceed process duration`);
  return timings;
}
