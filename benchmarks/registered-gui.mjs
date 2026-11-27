import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, unlink, writeFile } from "node:fs/promises";
import os from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { parseArgs } from "node:util";
import { consumerCommands, consumerPaths, publicationFixture, root, runConsumer, verifiedObservations } from "../scripts/registered-gui-consumers.mjs";
import { distribution, readTimings } from "./registered-gui-stats.mjs";

function count(text, min, max, name) {
  const n = Number(text);
  assert.ok(/^\d+$/.test(text) && Number.isInteger(n) && n >= min && n <= max,
    `${name} must be an integer from ${min} to ${max}`);
  return n;
}
const { values } = parseArgs({ options: {
  warmup: { type: "string", default: "3" }, samples: { type: "string", default: "30" },
  help: { type: "boolean", short: "h" },
} });
if (values.help) {
  console.log("Usage: node benchmarks/registered-gui.mjs [--warmup 3] [--samples 30]");
  process.exit(0);
}
const warmup = count(values.warmup, 0, 100, "warmup");
const samples = count(values.samples, 1, 500, "samples");
const fixture = JSON.parse(await readFile(publicationFixture, "utf8"));
assert.equal(fixture.version, 1);
const expected = new Map(fixture.cases.map(({ id, expected }) => [id, expected]));
assert.ok(expected.size > 0 && expected.size === fixture.cases.length);

const pathKey = (path) => relative(root, path).split(sep).join("/");
const sha256 = async (path) => createHash("sha256").update(await readFile(path)).digest("hex");
const inputs = { [pathKey(publicationFixture)]: await sha256(publicationFixture) };
for (const key of ["program", "read_draft", "read_publication"]) {
  // The reordered working fixture lives under target; keep using the canonical program bytes.
  const path = resolve(dirname(publicationFixture), fixture[key]);
  fixture[key] = path;
  inputs[pathKey(path)] = await sha256(path);
}

const parent = join(root, "target/benchmarks");
await mkdir(parent, { recursive: true });
const output = await mkdtemp(join(parent, "registered-gui."));
console.log(`Benchmark output: ${output}`);
const roundFixture = join(output, "workload.json");
const commands = consumerCommands(roundFixture, true);
const version = (command, args) => execFileSync(command, args, { cwd: root, encoding: "utf8" }).trim();
function git(args) {
  const run = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  return run.status === 0 ? run.stdout.trim() : null;
}

const paths = consumerPaths(true);
const binaries = [paths.rust, paths.worker, paths.nodeScript, join(dirname(paths.dotnet), "LeseLang.Consumer.dll")];
const artifacts = {};
for (const path of binaries) artifacts[pathKey(path)] = await sha256(path);
const sdk = join(root, "packages/javascript/dist");
const sdkHash = createHash("sha256");
for (const file of (await readdir(sdk, { recursive: true })).filter((f) => f.endsWith(".js")).sort()) {
  sdkHash.update(file.split(sep).join("/") + "\0").update(await readFile(join(sdk, file))).update("\0");
}
artifacts["packages/javascript/dist (*.js)"] = sdkHash.digest("hex");
const status = git(["status", "--porcelain"]);
const environment = {
  platform: process.platform, arch: process.arch, kernel: os.release(),
  cpu: os.cpus()[0]?.model ?? "unknown", logical_cpus: os.cpus().length, memory_bytes: os.totalmem(),
  node: process.version, rustc: version("rustc", ["--version"]), dotnet_sdk: version("dotnet", ["--version"]),
  dotnet_runtimes: version("dotnet", ["--list-runtimes"]).split(/\r?\n/)
    .filter((r) => r.startsWith("Microsoft.NETCore.App ")).map((r) => r.split(" [")[0]).sort(),
};
const summary = {
  schema_version: 1, suite: "registered-gui-publication", method_version: 1,
  started_utc: new Date().toISOString(), revision: git(["rev-parse", "HEAD"]),
  dirty_worktree: status === null ? null : status !== "", environment,
  inputs_sha256: inputs, artifacts_sha256: artifacts,
  configuration: { warmup, samples, rust_profile: "release", dotnet_configuration: "Release",
    order: "cyclic-hosts-and-cases" },
};

function rotate(items, step) {
  const first = step % items.length;
  return [...items.slice(first), ...items.slice(0, first)];
}
const rows = [];
try {
  for (let index = 0; index < warmup + samples; index++) {
    const measured = index >= warmup;
    const round = measured ? index - warmup : index;
    const cases = rotate(fixture.cases, round);
    await writeFile(roundFixture, JSON.stringify({ ...fixture, cases }));
    for (const command of rotate(commands, round)) {
      const [consumer] = command;
      const start = process.hrtime.bigint();
      const stdout = runConsumer(command);
      const suiteNs = Number(process.hrtime.bigint() - start);
      verifiedObservations(stdout, expected, consumer);
      const timings = readTimings(stdout, cases.map((c) => c.id), consumer, suiteNs);
      if (measured) rows.push({ round: round + 1, consumer, suite_process_ns: suiteNs, cases: Object.fromEntries(timings) });
    }
    if (!measured && index + 1 === warmup) console.log(`Validated ${warmup} warm-up round(s).`);
    if (measured && ((round + 1) % 10 === 0 || round + 1 === samples)) console.log(`Measured ${round + 1}/${samples} rounds.`);
  }
} finally {
  await unlink(roundFixture).catch((err) => { if (err.code !== "ENOENT") throw err; });
}
const results = {};
for (const [name] of commands) {
  const runs = rows.filter((r) => r.consumer === name);
  assert.equal(runs.length, samples);
  const cases = {};
  for (const id of expected.keys()) {
    const timings = {};
    for (const metric of ["primary_ns", "request_span_ns", "readback_ns"]) {
      timings[metric] = distribution(runs.map((r) => r.cases[id][metric]));
    }
    cases[id] = timings;
  }
  results[name] = { suite_process_ns: distribution(runs.map((r) => r.suite_process_ns)), cases };
  console.log(`${name}: suite p50 ${(results[name].suite_process_ns.p50 / 1e6).toFixed(2)} ms`);
}
const raw = rows.map((r) => JSON.stringify(r)).join("\n") + "\n";
await writeFile(join(output, "samples.ndjson"), raw);
const samples_sha256 = createHash("sha256").update(raw).digest("hex");
await writeFile(join(output, "summary.json"), JSON.stringify({ ...summary, samples_sha256, results }, null, 2) + "\n");
