import assert from "node:assert/strict";
import test from "node:test";
import { compareSummaries } from "./compare-registered-gui.mjs";

function example() {
  const stats = p50 => ({ p50, p95: null });
  const result = () => ({
    suite_process_ns: stats(100), cases: { publish: {
      primary_ns: stats(45.5), request_span_ns: stats(0), readback_ns: stats(10),
    } },
  });
  return {
    schema_version: 1, suite: "registered-gui-publication", method_version: 1,
    environment: { platform: "darwin", arch: "arm64", cpu: "example", logical_cpus: 8, memory_bytes: 8192,
      kernel: "1.0", node: "24.6.0", rustc: "1.98.0", dotnet_sdk: "10.0.201", dotnet_runtimes: [".NET 10.0.5"] },
    inputs_sha256: { fixture: "a".repeat(64) },
    configuration: { warmup: 1, samples: 2, rust_profile: "release", dotnet_configuration: "Release",
      order: "cyclic-hosts-and-cases" },
    results: { rust: result(), node: result(), dotnet: result() },
  };
}

test("compares each Consumer and stage while allowing a different build", () => {
  const baseline = example(), current = example();
  current.revision = "new-revision";
  current.artifacts_sha256 = { binary: "b".repeat(64) };
  current.results.node.cases.publish.readback_ns.p50 = 15;
  current.results.rust.suite_process_ns.p50 = 90;
  const rows = compareSummaries(baseline, current);
  assert.equal(rows.length, 12);
  assert.deepEqual(rows.find(([h, , m]) => h === "node" && m === "readback_ns"),
    ["node", "publish", "readback_ns", 10, 15, 5, 1.5]);
  assert.deepEqual(rows[0], ["rust", "(suite)", "suite_process_ns", 100, 90, -10, 0.9]);
  assert.ok(rows.filter(([, , m]) => m === "request_span_ns").every(row => row[6] === null));
});

test("incompatible runs and invalid timings fail before reporting a comparison", () => {
  for (const change of [
    r => { r.method_version = 2; },
    r => { r.inputs_sha256.fixture = "c".repeat(64); },
    r => { r.environment.node = "24.6.1"; },
    r => { r.configuration.samples = 4; },
    r => { delete r.results.dotnet; },
    r => { delete r.results.node.cases.publish; },
    r => { r.results.rust.suite_process_ns.p50 = -1; },
    r => { delete r.results.dotnet.cases.publish.primary_ns; },
    r => { r.results.dotnet.cases.publish.readback_ns.p95 = 12; },
  ]) {
    const current = example();
    change(current);
    assert.throws(() => compareSummaries(example(), current));
  }
});
