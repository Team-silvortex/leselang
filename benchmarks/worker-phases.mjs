import assert from "node:assert/strict";
import { recordsWithPrefix } from "../scripts/registered-gui-consumers.mjs";
import { distribution } from "./registered-gui-stats.mjs";

const sessions = ["primary", "read_draft", "read_publication"];
const isCount = (n) => Number.isSafeInteger(n) && n >= 0;

// Only durations are combined across processes. The ready handshake and terminal
// report enclose all worker timers on the host's wall clock.
function components(s) {
  const out = { spawn_call_ns: s.spawn_call_ns, ready_wait_ns: s.startup_ns - s.spawn_call_ns,
    shutdown_ns: s.shutdown_ns };
  const active = s.wall_ns - s.startup_ns - s.shutdown_ns;
  if (s.worker_profile === null) return { ...out, interrupted_active_ns: active };
  const { compile_ns, vm_local_ns, exchange_ns } = s.worker_profile;
  return { ...out, compile_ns, vm_local_ns, handler_ns: s.handler_ns,
    protocol_ns: exchange_ns - s.handler_ns,
    session_other_ns: active - compile_ns - vm_local_ns - exchange_ns };
}

export function readWorkerPhases(stdout, cases, consumer, suiteNs) {
  const records = recordsWithPrefix(stdout, "LESE_GUI_PHASES ", consumer);
  assert.deepEqual([...records.keys()], cases.map((c) => c.id), `${consumer}: missing or out-of-order phase records`);
  let wall = 0;
  for (const { id, expected } of cases) {
    const { id: reportedId, ...row } = records.get(id);
    assert.deepEqual(Object.keys(row).sort(), [...sessions].sort(), `${reportedId}: session set`);
    for (const role of sessions) {
      const s = row[role], label = `${consumer}/${id}/${role}`;
      assert.ok([s.wall_ns, s.startup_ns, s.spawn_call_ns, s.shutdown_ns, s.handler_ns].every(isCount) && s.wall_ns > 0,
        `${label}: invalid host duration`);
      const activeNs = s.wall_ns - s.startup_ns - s.shutdown_ns;
      assert.ok(s.spawn_call_ns <= s.startup_ns && s.handler_ns <= activeNs, `${label}: invalid lifetime`);
      // Each read has five bootstrap requests, two program Effects and two final reads.
      assert.equal(s.request_count, role === "primary" ? expected.trace.length : 9, `${label}: request count`);
      const interrupted = role === "primary" && expected.outcome === "cancelled";
      assert.equal(s.worker_profile === null, interrupted, `${label}: unexpected or missing worker telemetry`);
      if (!interrupted) {
        const p = s.worker_profile;
        assert.ok([p.compile_ns, p.vm_local_ns, p.exchange_ns].every(isCount), `${label}: invalid worker duration`);
        assert.equal(p.exchange_count, s.request_count, `${label}: unpaired exchanges`);
        assert.ok(Object.values(components(s)).every(isCount), `${label}: overlapping or out-of-window phases`);
      }
      wall += s.wall_ns;
    }
    records.set(id, row);
  }
  assert.ok(wall <= suiteNs, `${consumer}: worker lifetimes exceed suite wall time`);
  return records;
}

function distributions(rows) {
  return Object.fromEntries(Object.keys(rows[0]).map((key) => [key, distribution(rows.map((row) => row[key]))]));
}

export function summarizeWorkerPhases(runs) {
  const rounds = [];
  let roundTrips = 0, exchangeNs = 0, protocolNs = 0;
  for (const run of runs) {
    let wall = 0;
    const budget = {};
    for (const row of Object.values(run.cases)) {
      for (const s of Object.values(row)) {
        wall += s.wall_ns;
        const parts = components(s);
        for (const [key, value] of Object.entries(parts)) budget[key] = (budget[key] ?? 0) + value;
        if (s.worker_profile) {
          roundTrips += s.worker_profile.exchange_count;
          exchangeNs += s.worker_profile.exchange_ns;
          protocolNs += parts.protocol_ns;
        }
      }
    }
    budget.consumer_other_ns = run.suite_process_ns - wall;
    assert.equal(Object.values(budget).reduce((a, b) => a + b, 0), run.suite_process_ns);
    rounds.push(budget);
  }
  const totalNs = runs.reduce((sum, r) => sum + r.suite_process_ns, 0);
  const attribution = distributions(rounds);
  for (const key of Object.keys(attribution)) {
    attribution[key].percent_of_suite = 100 * rounds.reduce((sum, r) => sum + r[key], 0) / totalNs;
  }
  const cases = {};
  for (const id of Object.keys(runs[0].cases)) {
    const roles = {};
    for (const role of sessions) {
      const rows = runs.map((r) => r.cases[id][role]);
      roles[role] = distributions(rows.map((s) => ({ wall_ns: s.wall_ns, startup_ns: s.startup_ns, ...components(s) })));
      if (rows[0].worker_profile) {
        roles[role].exchange_ns = distribution(rows.map((s) => s.worker_profile.exchange_ns));
      }
    }
    cases[id] = roles;
  }
  return { attribution, completed_session_exchanges: { count: roundTrips,
    mean_rtt_ns: exchangeNs / roundTrips, mean_protocol_ns: protocolNs / roundTrips }, cases };
}
