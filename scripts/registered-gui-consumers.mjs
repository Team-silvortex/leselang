import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const root = fileURLToPath(new URL("../", import.meta.url));
export const publicationFixture = join(root, "fixtures/registered-gui/publication.json");

export function consumerPaths(benchmark) {
  const suffix = process.platform === "win32" ? ".exe" : "";
  const native = join(root, "target", benchmark ? "release" : "lean");
  const dotnet = join(root, "target/dotnet/bin/LeseLang.Consumer", benchmark ? "release" : "debug");
  return {
    rust: join(native, "leselang-registered-host-consumer" + suffix),
    worker: join(native, "leselang-agent-worker" + suffix),
    nodeScript: join(root, "consumer/javascript/agent/registered-gui.mjs"),
    dotnet: join(dotnet, "LeseLang.Consumer" + suffix),
  };
}

export function consumerCommands(fixture, mode = "--registered-gui") {
  const benchmark = mode !== "--registered-gui";
  const { rust, worker, nodeScript, dotnet } = consumerPaths(benchmark);
  const commands = [
    ["node", process.execPath, [nodeScript, ...(benchmark ? [mode] : []), worker, fixture]],
    ["dotnet", dotnet, [mode, worker, fixture]],
  ];
  if (mode !== "--profile-registered-gui") commands.unshift(["rust", rust, [mode, fixture]]);
  return commands;
}

export function runConsumer([name, executable, args]) {
  const run = spawnSync(executable, args, { encoding: "utf8", cwd: root, timeout: 180_000 });
  if (run.error) throw run.error;
  if (run.status !== 0) throw new Error(`${name} exited with ${run.signal ?? run.status}\n${run.stderr.slice(-4000)}`);
  return run.stdout;
}

export function recordsWithPrefix(stdout, prefix, name) {
  const records = new Map();
  for (const line of stdout.split(/\r?\n/)) {
    if (!line.startsWith(prefix)) continue;
    const record = JSON.parse(line.slice(prefix.length));
    assert.ok(typeof record.id === "string" && !records.has(record.id), `${name}: invalid or repeated id ${record.id}`);
    records.set(record.id, record);
  }
  return records;
}

export function verifiedObservations(stdout, expected, name) {
  const records = recordsWithPrefix(stdout, "LESE_GUI ", name);
  const observed = new Map(Array.from(records, ([id, record]) => [id, record.observed]));
  assert.deepEqual(observed, expected, `${name}: missing case or observations differ from fixture`);
  return observed;
}
