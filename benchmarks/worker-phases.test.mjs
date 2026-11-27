import assert from "node:assert/strict";
import test from "node:test";
import { readWorkerPhases, summarizeWorkerPhases } from "./worker-phases.mjs";

const complete = () => ({ wall_ns: 100, startup_ns: 20, spawn_call_ns: 12, shutdown_ns: 10, handler_ns: 8, request_count: 9,
  worker_profile: { compile_ns: 7, vm_local_ns: 6, exchange_ns: 50, exchange_count: 9 } });
const row = () => ({ id: "case", primary: complete(), read_draft: complete(), read_publication: complete() });
function read(record, outcome = "reported") {
  return Object.fromEntries(readWorkerPhases("LESE_GUI_PHASES " + JSON.stringify(record),
    [{ id: record.id, expected: { outcome, trace: Array(9).fill("request") } }], "node", 350));
}

test("the additive budget counts callbacks once, closes to suite wall and keeps RTT inclusive", () => {
  const cases = read(row());
  const r = summarizeWorkerPhases([{ suite_process_ns: 350, cases }]);
  assert.equal(r.attribution.spawn_call_ns.p50, 36);
  assert.equal(r.attribution.ready_wait_ns.p50, 24);
  assert.equal(r.cases.case.primary.startup_ns.p50, 20);
  assert.equal(r.cases.case.primary.exchange_ns.p50, 50);
  assert.equal(r.attribution.protocol_ns.p50, 126);
  assert.equal(r.attribution.session_other_ns.p50, 21);
  assert.equal(r.attribution.consumer_other_ns.p50, 50);
  assert.equal(r.attribution.handler_ns.p50, 24);
  assert.equal(r.completed_session_exchanges.mean_rtt_ns, 150 / 27);
  const share = Object.values(r.attribution).reduce((sum, part) => sum + part.percent_of_suite, 0);
  assert.ok(Math.abs(100 - share) < 1e-10);
});

test("killed workers contribute observed lifecycle and an explicitly unpartitioned active interval", () => {
  const record = row();
  record.primary.worker_profile = null;
  assert.throws(() => read(record));
  const r = summarizeWorkerPhases([{ suite_process_ns: 350, cases: read(record, "cancelled") }]);
  assert.equal(r.attribution.interrupted_active_ns.p50, 70);
  assert.equal(r.cases.case.primary.compile_ns, undefined);
  assert.equal(r.cases.case.primary.exchange_ns, undefined);
  assert.equal(r.completed_session_exchanges.count, 18);
  assert.equal(r.attribution.handler_ns.p50, 16);
  record.read_publication.worker_profile = null;
  assert.throws(() => read(record, "cancelled"));
});

test("a mismatched exchange count, nested span overflow or impossible session total fails the run", () => {
  for (const change of [
    (r) => { r.primary.worker_profile.exchange_count = 8; },
    (r) => { r.primary.handler_ns = 51; },
    (r) => { r.primary.spawn_call_ns = 21; },
    (r) => { r.primary.worker_profile.compile_ns = 71; },
    (r) => { r.primary.wall_ns = 151; },
  ]) {
    const record = row();
    change(record);
    assert.throws(() => read(record));
  }
});
