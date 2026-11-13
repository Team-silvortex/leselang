import assert from "node:assert/strict";
import test from "node:test";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", timeout: 30_000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

test("packed public ESM and declarations work in an external JavaScript/TypeScript consumer", async () => {
  const directory = await mkdtemp(join(tmpdir(), "leselang-bridge-package-"));
  try {
    const cache = join(directory, "npm-cache");
    const [packed] = JSON.parse(run("npm", [
      "pack", "--ignore-scripts", "--offline", "--json", "--pack-destination", directory, "--cache", cache,
    ], root));
    assert.deepEqual(packed.files.map((entry) => entry.path).sort(), [
      "LICENSE", "README.md", "dist/agent.d.ts", "dist/agent.js", "dist/index.d.ts", "dist/index.js",
      "dist/semantic-provider.d.ts", "dist/semantic-provider.js", "dist/semantics.d.ts", "dist/semantics.js", "package.json",
    ]);
    await writeFile(join(directory, "package.json"), JSON.stringify({ name: "external-bridge-consumer", private: true, type: "module" }));
    run("npm", [
      "install", join(directory, packed.filename), "--offline", "--ignore-scripts", "--no-audit", "--no-fund", "--cache", cache,
    ], directory);
    const installed = JSON.parse(await readFile(join(directory, "node_modules/@leselang/bridge/package.json"), "utf8"));
    assert.equal(installed.private, true);
    assert.equal(installed.license, "MIT");
    assert.deepEqual(installed.dependencies ?? {}, {});

    await writeFile(join(directory, "consumer.mjs"), `
import assert from "node:assert/strict";
import { Bridge, AgentSession, prepareSemanticCatalog, createSemanticProvider } from "@leselang/bridge";
assert.equal(typeof prepareSemanticCatalog, "function");
assert.equal(typeof createSemanticProvider().plan, "function");
assert.throws(() => prepareSemanticCatalog('{}'));
const scope = { window: "node-main", epoch: "instance-1", session: "task-1" };
const bridge = new Bridge(scope);
const state = { role: "button", title: "Increment", value: "0", enabled: true, editable: false, clicks: 0 };
bridge.register("counter", {
  read: () => state,
  async click(context) {
    await Promise.resolve();
    context.commit(() => { state.clicks++; state.value = String(state.clicks); });
  },
});
let request = 0;
const call = (operation, arguments_ = {}) => bridge.handle({ transport_version: 1, ...scope, request_id: ++request, budget_ms: 1000, operation, arguments: arguments_ });
const target = (await call("gui.find", { name: "counter" })).value;
const before = (await call("gui.observe", { target })).value;
assert.equal((await call("gui.click", { target, expected_revision: before.revision })).status, "ok");
const after = (await call("gui.observe", { target })).value;
assert.equal(after.clicks, 1);
assert.equal(after.value, "1");
assert.equal(before.value, "0");
await assert.rejects(import("@leselang/bridge/src/index.js"), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
bridge.close();
assert.equal((await call("gui.list")).status, "rejected");
const session = new AgentSession({
  provider: { async plan({ message }) { return { summary: "Save", source: "fn main(text: string) = true", arguments: { text: message } }; } },
  host: {
    async describe() { return { profile: "external" }; },
    async execute(plan) { return { saved: plan.arguments.text }; },
    verify(report, plan) { return report.saved === plan.arguments.text; },
  },
});
assert.equal((await session.submit("literal data")).status, "verified");
`);
    run(process.execPath, ["consumer.mjs"], directory);

    await writeFile(join(directory, "consumer.ts"), `
import { Bridge, AgentSession, type Scope, type Adapter, type ActionContext, type Reply,
  type AgentProvider, type AgentHost, type AgentOutcome, type WorkflowPlan,
  prepareSemanticCatalog, createSemanticProvider, type SemanticCatalog, type SemanticTaskInfo,
  type PreparedSemanticTask, type SemanticProviderOptions } from "@leselang/bridge";
const semanticOptions: SemanticProviderOptions = { aliases: { save: "publish" } };
const semanticProvider: AgentProvider = createSemanticProvider(semanticOptions);
const compileCatalog: (data: unknown) => SemanticCatalog = prepareSemanticCatalog;
const describeTask = (task: SemanticTaskInfo): string => task.id;
const candidate = (task: PreparedSemanticTask): WorkflowPlan => task.plan;
void semanticProvider; void compileCatalog; void describeTask; void candidate;
const scope: Scope = { window: "worker", epoch: "instance-1", session: "task-1" };
const bridge = new Bridge(scope);
const adapter: Adapter = {
  read: () => ({ role: "button", title: "Increment", value: "0", enabled: true, editable: false, clicks: 0 }),
  async click(context: ActionContext) { await Promise.resolve(); context.commit(() => undefined); },
};
bridge.register("counter", adapter);
const reply: Promise<Reply> = bridge.handle({});
void reply;
const provider: AgentProvider = { async plan({ message }) {
  return { summary: "Save", source: "fn main(text: string) = true", arguments: { text: message } };
} };
const host: AgentHost = { async describe() { return {}; }, async execute(plan) { return { saved: plan.arguments.text }; }, verify() { return true; } };
const session = new AgentSession({ provider, host });
const outcome: Promise<AgentOutcome> = session.submit("task");
void outcome;
// @ts-expect-error task arguments remain strings
const invalidPlan: WorkflowPlan = { summary: "Save", source: "source", arguments: { text: 42 } };
void invalidPlan;
// @ts-expect-error endpoint labels are strings
const invalidScope: Scope = { window: 42, epoch: "page", session: "task" };
// @ts-expect-error an adapter must return typed scalar state
const invalidAdapter: Adapter = { read: () => ({ role: "button", title: "", value: "", enabled: "yes", editable: false, clicks: 0 }) };
// @ts-expect-error source implementation is not a public package export
import { Bridge as PrivateBridge } from "@leselang/bridge/src/index.js";
void invalidScope; void invalidAdapter; void PrivateBridge;
`);
    run("tsc", ["--noEmit", "--strict", "--target", "ES2022", "--module", "NodeNext", "--moduleResolution", "NodeNext", "consumer.ts"], directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
