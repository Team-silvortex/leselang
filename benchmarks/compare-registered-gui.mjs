import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";

const hosts = ["rust", "node", "dotnet"];
const metrics = ["primary_ns", "request_span_ns", "readback_ns"];

function validateSummary(summary, label) {
  assert.equal(summary.schema_version, 1, `${label}: unknown summary schema`);
  assert.equal(summary.suite, "registered-gui-publication", `${label}: unexpected benchmark suite`);
  assert.equal(summary.method_version, 1, `${label}: unknown measurement method`);
  const config = summary.configuration;
  assert.equal(config.rust_profile, "release");
  assert.equal(config.dotnet_configuration, "Release");
  assert.equal(config.order, "cyclic-hosts-and-cases");
  assert.ok(Number.isSafeInteger(config.warmup) && config.warmup >= 0, `${label}: invalid warmup count`);
  assert.ok(Number.isSafeInteger(config.samples) && config.samples > 0, `${label}: invalid sample count`);

  const env = summary.environment;
  for (const key of ["platform", "arch", "cpu", "kernel", "node", "rustc", "dotnet_sdk"]) {
    assert.ok(typeof env[key] === "string" && env[key].trim(), `${label}: missing environment.${key}`);
  }
  for (const key of ["memory_bytes", "logical_cpus"]) {
    assert.ok(Number.isSafeInteger(env[key]) && env[key] > 0, `${label}: invalid environment.${key}`);
  }
  const runtimes = env.dotnet_runtimes;
  assert.ok(Array.isArray(runtimes) && runtimes.length && runtimes.every(v => typeof v === "string" && v.trim()),
    `${label}: missing .NET runtimes`);
  const hashes = Object.entries(summary.inputs_sha256);
  assert.ok(hashes.length && hashes.every(([name, value]) => name && typeof value === "string" && /^[a-f0-9]{64}$/.test(value)),
    `${label}: invalid workload fingerprints`);
  assert.deepEqual(Object.keys(summary.results).sort(), [...hosts].sort(), `${label}: unexpected Consumer set`);
}

function nanoseconds(stats, samples, allowZero, label) {
  const valid = n => Number.isFinite(n) && n >= 0 && n <= Number.MAX_SAFE_INTEGER && (allowZero || n > 0);
  assert.ok(valid(stats?.p50), `${label}: invalid p50`);
  if (samples < 20) assert.equal(stats.p95, null, `${label}: p95 requires at least 20 samples`);
  else assert.ok(valid(stats.p95) && stats.p95 >= stats.p50, `${label}: invalid p95`);
  return stats.p50;
}

export function compareSummaries(baseline, current) {
  validateSummary(baseline, "baseline");
  validateSummary(current, "current");
  for (const key of ["inputs_sha256", "configuration", "environment"]) {
    assert.deepEqual(current[key], baseline[key], `incompatible ${key}`);
  }
  const ids = Object.keys(baseline.results.rust.cases).sort();
  assert.ok(ids.length && ids.every(Boolean), "empty or missing case ID");
  const rows = [];
  function add(host, id, metric, before, after) {
    const label = `${host}/${id}/${metric}`;
    const samples = baseline.configuration.samples;
    const zeroOK = metric === "request_span_ns";
    const oldNs = nanoseconds(before, samples, zeroOK, `baseline ${label}`);
    const newNs = nanoseconds(after, samples, zeroOK, `current ${label}`);
    rows.push([host, id, metric, oldNs, newNs, newNs - oldNs, oldNs === 0 ? null : newNs / oldNs]);
  }
  for (const host of hosts) {
    const before = baseline.results[host];
    const after = current.results[host];
    assert.deepEqual(Object.keys(before.cases).sort(), ids, `baseline ${host}: different case set`);
    assert.deepEqual(Object.keys(after.cases).sort(), ids, `current ${host}: different case set`);
    add(host, "(suite)", "suite_process_ns", before.suite_process_ns, after.suite_process_ns);
    for (const id of ids) {
      for (const metric of metrics) add(host, id, metric, before.cases[id][metric], after.cases[id][metric]);
    }
  }
  return rows;
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    const files = process.argv.slice(2);
    assert.equal(files.length, 2, "usage: node benchmarks/compare-registered-gui.mjs BASELINE.json CURRENT.json");
    const [before, after] = await Promise.all(files.map(async file => JSON.parse(await readFile(file, "utf8"))));
    const rows = compareSummaries(before, after);
    console.log("consumer\tcase\tmetric\tbaseline_p50_ns\tcurrent_p50_ns\tdelta_ns\tcurrent/baseline");
    for (const [host, id, metric, oldNs, newNs, delta, ratio] of rows) {
      console.log([host, id, metric, oldNs, newNs, delta, ratio === null ? "n/a" : ratio.toFixed(4)].join("\t"));
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
