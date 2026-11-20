// Acceptance from an installed tarball in a new directory, with the real VM.
// Only application modules are copied; the carrier comes from the installed package.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const sdk = fileURLToPath(new URL("../../../packages/javascript/", import.meta.url));
const worker = fileURLToPath(new URL("../../../target/lean/leselang-agent-worker", import.meta.url));

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", timeout: 30_000 });
  assert.ifError(result.error);
  assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}`);
  return result.stdout;
}

test("an external installed Node SDK consumer runs real workflows, partial failure and cancellation", { timeout: 60_000 }, async () => {
  const directory = await mkdtemp(join(tmpdir(), "leselang-node-package-"));
  try {
    const cache = join(directory, "empty-npm-cache");
    const [packed] = JSON.parse(run("npm", ["pack", "--ignore-scripts", "--offline", "--json",
      "--pack-destination", directory, "--cache", cache], sdk));
    await writeFile(join(directory, "package.json"), JSON.stringify({
      name: "external-node-agent-consumer", private: true, type: "module",
    }));
    run("npm", ["install", join(directory, packed.filename), "--offline", "--ignore-scripts",
      "--no-audit", "--no-fund", "--cache", cache], directory);
    const installed = JSON.parse(await readFile(join(directory, "node_modules/@leselang/bridge/package.json"), "utf8"));
    assert.deepEqual(installed.dependencies ?? {}, {}, "the public Node carrier has no runtime npm dependencies");
    assert.deepEqual((await readdir(join(directory, "node_modules"))).sort(), [".package-lock.json", "@leselang"]);

    await mkdir(join(directory, "agent"));
    for (const file of ["application.mjs", "catalog.mjs", "assistant.mjs"]) {
      await copyFile(new URL(file, import.meta.url), join(directory, "agent", file));
    }
    await copyFile(new URL("../semantic-contract.mjs", import.meta.url), join(directory, "semantic-contract.mjs"));
    await writeFile(join(directory, "consumer.mjs"), String.raw`
import assert from "node:assert/strict";
import { AgentWorkflowSession, prepareSemanticWorkflow } from "@leselang/bridge";
import { createNodeAgentTransport } from "@leselang/bridge/node";
import { createPublicationApplication } from "./agent/application.mjs";
import { createPublicationAgent } from "./agent/assistant.mjs";

assert.equal(typeof createNodeAgentTransport, "function");
assert.equal(Object.hasOwn(await import("@leselang/bridge"), "createNodeAgentTransport"), false);
await assert.rejects(import("@leselang/bridge/dist/node/index.js"), { code: "ERR_PACKAGE_PATH_NOT_EXPORTED" });
const executable = process.argv[2];
const report = (step) => JSON.parse(step.report);
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  const promise = new Promise((yes) => { resolve = yes; });
  return { promise, resolve };
}
function workflow(host, text) {
  return new AgentWorkflowSession({ host, provider: { async plan({ catalog }) {
    return prepareSemanticWorkflow(catalog, [
      { task: "read_draft", arguments: {} },
      { task: "publish_article", arguments: { text: { fromStep: 0, format: "string" } },
        requires: { fromStep: 0, format: "string", equals: text } },
      { task: "read_publication", arguments: {} },
      { task: "check_publication", arguments: {} },
    ]);
  } } });
}
const acceptance = {};
{
  const text = 'Packed external "应用" \\ path\nsecond line ✓';
  const application = createPublicationApplication({ initialDraft: text });
  let trace = "";
  const app = createPublicationAgent({ executable, application, onStderr: (chunk) => { trace += chunk; } });
  const session = workflow(app.host, text);
  try {
    const outcome = await session.submit("run the declared offline workflow");
    assert.equal(outcome.status, "verified", outcome.error);
    assert.equal(outcome.possiblePartialEffects, false);
    assert.deepEqual(outcome.steps.map((step) => report(step).effects), [2, 9, 2, 1]);
    assert.deepEqual(outcome.steps.map((step) => report(step).result), [
      { type: "string", value: text }, { type: "boolean", value: true },
      { type: "string", value: text }, { type: "none", value: null },
    ]);
    assert.equal(outcome.requirements[0].satisfied, true);
    assert.equal(outcome.requirements[0].actual, text);
    assert.equal(outcome.steps[1].plan.arguments.text, text);
    assert.equal(outcome.steps[1].plan.source.includes(text), false);
    assert.equal((trace.match(/poll -> authorize -> dispatch:/gu) ?? []).length, 14);
    assert.equal((trace.match(/^accept$/gmu) ?? []).length, 14);
    for (const step of outcome.steps) {
      const value = report(step);
      assert.equal(value.profile, "javascript.registered");
      assert.equal(value.kind, "javascript_task_run");
      assert.ok(value.fuel_remaining > 0 && value.fuel_remaining < 10_000);
      assert.equal(Object.hasOwn(value, "verified"), false);
    }
    assert.deepEqual(application.snapshot(), { draft: text, published: text, status: "published", publishes: 1, resets: 0, unrelated: 0 });
    acceptance.workflow = { status: outcome.status, effects: 14, publishes: application.snapshot().publishes };
  } finally { session.cancel(); app.close(); }
}
{
  const text = "already published through the installed package ✓";
  const application = createPublicationApplication({ initialDraft: text, faultAfterCommit: true });
  const app = createPublicationAgent({ executable, application });
  const session = workflow(app.host, text);
  try {
    const outcome = await session.submit("publish then verify");
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.deepEqual(outcome.steps.map((step) => step.status), ["verified", "failed"]);
    assert.equal(outcome.steps[1].report, undefined);
    assert.deepEqual(application.snapshot(), { draft: text, published: text, status: "published", publishes: 1, resets: 0, unrelated: 0 });
    const observed = await app.session.submit("task read_publication");
    assert.equal(observed.status, "verified", observed.error);
    assert.equal(report(observed).result.value, text);
    assert.equal(application.snapshot().publishes, 1, "failure never retries a committed action");
    acceptance.partial = { status: outcome.status, possiblePartialEffects: true, publishes: 1 };
  } finally { session.cancel(); app.close(); }
}
{
  const entered = deferred(), release = deferred();
  const application = createPublicationApplication({ initialDraft: "before", async beforePublish() {
    entered.resolve(); await release.promise;
  } });
  const app = createPublicationAgent({ executable, application });
  try {
    const pending = app.session.submit("task publish_article:earlier committed draft");
    await Promise.race([entered.promise, pending.then((outcome) => assert.fail("action not entered: " + outcome.error))]);
    assert.equal(app.session.cancel(), true);
    const outcome = await pending;
    assert.equal(outcome.status, "cancelled");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(application.snapshot().draft, "earlier committed draft");
    assert.equal(application.snapshot().publishes, 0);
    release.resolve(); await tick(); await tick();
    assert.equal(application.snapshot().publishes, 0, "a late continuation cannot commit");
    const read = await app.session.submit("task read_draft");
    assert.equal(read.status, "verified", read.error);
    assert.equal(report(read).result.value, "earlier committed draft");
    acceptance.cancelled = { status: outcome.status, possiblePartialEffects: true, publishes: 0, subsequentRead: read.status };
  } finally { release.resolve(); app.close(); }
}
process.stdout.write(JSON.stringify(acceptance) + "\n");
`);
    const acceptance = JSON.parse(run(process.execPath, ["consumer.mjs", worker], directory));
    assert.deepEqual(acceptance, {
      workflow: { status: "verified", effects: 14, publishes: 1 },
      partial: { status: "failed", possiblePartialEffects: true, publishes: 1 },
      cancelled: { status: "cancelled", possiblePartialEffects: true, publishes: 0, subsequentRead: "verified" },
    });

    await writeFile(join(directory, "consumer.ts"), `
import { createNodeAgentTransport, type NodeAgentTransportOptions } from "@leselang/bridge/node";
import { createRegisteredAgentHost, createRegisteredApplication, type RegisteredAgentTransport } from "@leselang/bridge";
const options: NodeAgentTransportOptions = {
  executable: "/trusted/agent_worker", args: [], timeoutMs: 15_000,
  catalog(base: string): string { return base; },
  onStderr(text: string): void { void text; },
};
const transport: RegisteredAgentTransport = createNodeAgentTransport(options);
const host = createRegisteredAgentHost({
  window: "typed-node", epoch: "fresh-instance", transport,
  application: createRegisteredApplication({}),
  observations: { before: "objects_before", after: "objects_after" },
});
void host;
// @ts-expect-error worker paths are strings
const badExecutable: NodeAgentTransportOptions = { executable: 123, catalog: (base) => base };
// @ts-expect-error only the optional Node subpath exports a process carrier
import { createNodeAgentTransport as rootCarrier } from "@leselang/bridge";
void badExecutable; void rootCarrier;
`);
    await writeFile(join(directory, "tsconfig.json"), JSON.stringify({
      compilerOptions: { noEmit: true, strict: true, target: "ES2022", module: "NodeNext", moduleResolution: "NodeNext",
        lib: ["ES2022", "DOM"], types: [], allowJs: true, checkJs: true, noUncheckedIndexedAccess: true },
      files: ["consumer.ts", "semantic-contract.mjs", "agent/catalog.mjs", "agent/application.mjs"],
    }));
    run(process.execPath, [join(sdk, "node_modules/typescript/bin/tsc"), "--project", "tsconfig.json"], directory);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
