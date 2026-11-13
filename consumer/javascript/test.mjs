// The Rust example must be built explicitly first. These are separate-process
// acceptance tests, not mocks of Bridge or the language VM.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { AgentSession, createSemanticProvider, prepareSemanticCatalog } from "@leselang/bridge";
import { publicationCatalog, publicationTask } from "./semantic-contract.mjs";

const executable = fileURLToPath(new URL("../../target/lean/examples/node_host", import.meta.url));
const source = new URL("publish.lese", import.meta.url);
function run(text, ...args) {
  const result = spawnSync(executable, ["--text", text, ...args], {
    encoding: "utf8", timeout: 15_000, maxBuffer: 128 * 1024,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, result.stderr);
  return result;
}
function stateEvidence(stderr) {
  return stderr.split("\n").filter((line) => line.startsWith('{"kind":"javascript_consumer_state"'))
    .map((line) => JSON.parse(line));
}

test("one compiled LeseLang task controls five application-owned JS objects with literal inputs", () => {
  const original = readFileSync(source, "utf8");
  for (const text of ['JavaScript "保存" \\ 路径\n第二行 ✓', '--fault-after-commit']) {
    const result = run(text);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trimEnd().split("\n").length, 1);
    const report = JSON.parse(result.stdout);
    assert.equal(report.kind, "javascript_run");
    assert.equal(report.profile, "javascript.registered");
    assert.deepEqual(report.result, { type: "boolean", value: true });
    assert.equal(report.verified, true);
    assert.equal(report.effects, 9);
    assert.ok(report.fuel_remaining > 0 && report.fuel_remaining < 10_000);
    assert.deepEqual(report.objects_after.map((item) => item.identifier), ["unrelated", "status", "draft", "reset", "publish"]);
    const get = (items, name) => items.find((item) => item.identifier === name);
    assert.equal(get(report.objects_after, "draft").value, text);
    assert.equal(get(report.objects_after, "publish").value, text);
    assert.equal(get(report.objects_after, "publish").clicks, 1);
    assert.equal(get(report.objects_after, "status").value, "published");
    for (const name of ["reset", "unrelated"]) {
      assert.deepEqual(get(report.objects_after, name), get(report.objects_before, name));
    }
    assert.deepEqual(stateEvidence(result.stderr), [{ kind: "javascript_consumer_state",
      published: text, publishes: 1, resets: 0, unrelated: 0, status: "published" }]);
  }
  assert.equal(readFileSync(source, "utf8"), original, "task text never becomes generated source");
});

test("an exception after the JS commit retains publication and fails without retry", () => {
  const text = "The publication happened before this failure ✓";
  const result = run(text, "--fault-after-commit");
  assert.equal(result.status, 1, result.stderr);
  assert.equal(result.stdout, "", "no success report may be published");
  assert.match(result.stderr, /outcome may be partial; no retry/);
  assert.deepEqual(stateEvidence(result.stderr), [{ kind: "javascript_consumer_state",
    published: text, publishes: 1, resets: 0, unrelated: 0, status: "published" }]);
});

test("oversized task input is refused before JavaScript application work", () => {
  const result = run("界".repeat(1366));
  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /4096 UTF-8 bytes/);
  assert.deepEqual(stateEvidence(result.stderr), []);
});

test("offline catalog discovery needs no Node child and rejects execution arguments", () => {
  const description = spawnSync(executable, ["--describe"], {
    encoding: "utf8", env: { ...process.env, PATH: "" }, timeout: 5_000,
  });
  assert.ifError(description.error);
  assert.equal(description.status, 0, description.stderr);
  const catalog = JSON.parse(description.stdout);
  assert.equal(catalog.profile, "javascript.registered");
  assert.equal(catalog.operations.length, 5);
  assert.equal(description.stderr, "");
  for (const args of [["--describe", "--fault-after-commit"], ["--unknown"]]) {
    const result = spawnSync(executable, args, { encoding: "utf8", timeout: 5_000 });
    assert.ifError(result.error);
    assert.equal(result.status, 1);
    assert.equal(result.stdout, "");
    assert.deepEqual(stateEvidence(result.stderr), []);
  }
});

function describedPublication() {
  const result = spawnSync(executable, ["--describe"], {
    encoding: "utf8", env: { ...process.env, PATH: "" }, timeout: 5_000,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
  return publicationCatalog(JSON.parse(result.stdout));
}

function semanticSession(directory, { fault = false, expectStatus } = {}) {
  let raw = describedPublication();
  if (expectStatus) {
    const declaration = JSON.parse(raw);
    declaration.semantics.tasks[0].verification.predicates
      .find(({ id }) => id === "publication_status").right.value.value = expectStatus;
    raw = JSON.stringify(declaration);
  }
  // Application-owned declaration and verifier are admitted independently of
  // the provider. They are never taken from provider-added plan metadata.
  const applicationCatalog = prepareSemanticCatalog(raw);
  const executions = [];
  const session = new AgentSession({
    provider: createSemanticProvider(),
    host: {
      async describe() { return raw; },
      async execute(plan) {
        const prepared = applicationCatalog.prepare(plan.task, plan.arguments);
        assert.equal(plan.source, prepared.plan.source);
        assert.equal(plan.task, publicationTask);
        const filename = join(directory, `generated-${executions.length}.lese`);
        writeFileSync(filename, plan.source, { encoding: "utf8", flag: "wx", mode: 0o600 });
        const result = run(plan.arguments.text, "--run", filename, ...(fault ? ["--fault-after-commit"] : []));
        executions.push(result);
        if (result.status !== 0) throw new Error("Rust workflow failed; earlier application effects may remain; no retry");
        return JSON.parse(result.stdout);
      },
      verify(report, plan) {
        return applicationCatalog.prepare(plan.task, plan.arguments).verify(report);
      },
    },
  });
  return { session, executions, catalog: applicationCatalog };
}

test("the public semantic provider drives an independent five-control application through the real VM", async () => {
  const directory = mkdtempSync(join(tmpdir(), "leselang-js-semantic-"));
  try {
    const f = semanticSession(directory);
    assert.equal(f.catalog.profile, "javascript.registered");
    assert.deepEqual(f.catalog.tasks.map(({ id }) => id), [publicationTask]);
    const sources = [];
    for (const text of ['Article "发布" \\ path\nsecond line ✓', '--fault-after-commit']) {
      const outcome = await f.session.submit(`task ${publicationTask}:${text}`);
      assert.equal(outcome.status, "verified", outcome.error);
      assert.equal(outcome.possiblePartialEffects, false);
      assert.equal(outcome.plan.task, publicationTask);
      assert.equal(outcome.plan.arguments.text, text);
      assert.equal(outcome.report.effects, 9);
      assert.equal(outcome.report.result.value, true);
      assert.equal(outcome.report.verified, true);
      assert.ok(outcome.report.fuel_remaining > 0 && outcome.report.fuel_remaining < 10_000);
      assert.ok(outcome.plan.source.includes('"draft"'));
      assert.ok(outcome.plan.source.includes('"publish"'));
      assert.ok(!outcome.plan.source.includes(text), "user content is a typed argument, never source");
      sources.push(outcome.plan.source);
      const last = f.executions.at(-1);
      assert.equal((last.stderr.match(/poll -> authorize -> dispatch:/gu) ?? []).length, 9);
      assert.deepEqual(stateEvidence(last.stderr), [{ kind: "javascript_consumer_state", published: text,
        publishes: 1, resets: 0, unrelated: 0, status: "published" }]);
    }
    assert.equal(sources[0], sources[1]);
    const unsupported = await f.session.submit("task fill_and_save:do not guess another app's task");
    assert.equal(unsupported.status, "failed");
    assert.equal(unsupported.possiblePartialEffects, false);
    assert.equal(f.executions.length, 2, "undeclared tasks never reach the Rust process");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("a semantic provider workflow preserves an actual post-commit failure without retry", async () => {
  const directory = mkdtempSync(join(tmpdir(), "leselang-js-semantic-failure-"));
  try {
    const f = semanticSession(directory, { fault: true });
    const text = "Publication survives the failed receipt ✓";
    const outcome = await f.session.submit(`任务 ${publicationTask}：${text}`);
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.report, undefined);
    assert.equal(f.executions.length, 1);
    const result = f.executions[0];
    assert.equal(result.status, 1);
    assert.equal(result.stdout, "");
    assert.deepEqual(stateEvidence(result.stderr), [{ kind: "javascript_consumer_state", published: text,
      publishes: 1, resets: 0, unrelated: 0, status: "published" }]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("a successful real VM report cannot bypass the application's declared postcondition", async () => {
  const directory = mkdtempSync(join(tmpdir(), "leselang-js-postcondition-"));
  try {
    const f = semanticSession(directory, { expectStatus: "approved" });
    const outcome = await f.session.submit(`task ${publicationTask}:Publication needs approval`);
    assert.equal(outcome.status, "failed");
    assert.equal(outcome.error, "application_verification_failed");
    assert.equal(outcome.possiblePartialEffects, true);
    assert.equal(outcome.report.verified, true, "the transport's own result is still evidence, not the app's complete contract");
    assert.equal(outcome.report.result.value, true);
    assert.equal(f.executions.length, 1);
    assert.equal(f.executions[0].status, 0);
    assert.deepEqual(stateEvidence(f.executions[0].stderr), [{ kind: "javascript_consumer_state",
      published: "Publication needs approval", publishes: 1, resets: 0, unrelated: 0, status: "published" }]);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("custom source files are admitted before a JavaScript child can start", () => {
  const directory = mkdtempSync(join(tmpdir(), "leselang-js-source-"));
  try {
    for (const [name, bytes, diagnostic] of [
      ["oversized.lese", Buffer.alloc(16385, 0x20), /16384/],
      ["invalid-utf8.lese", Buffer.from([0xff]), /UTF-8/],
    ]) {
      const filename = join(directory, name);
      writeFileSync(filename, bytes);
      const result = spawnSync(executable, ["--run", filename], {
        encoding: "utf8", env: { ...process.env, PATH: "" }, timeout: 5_000,
      });
      assert.ifError(result.error);
      assert.equal(result.status, 1);
      assert.equal(result.stdout, "");
      assert.match(result.stderr, diagnostic);
      assert.doesNotMatch(result.stderr, /cannot start the JavaScript consumer/);
      assert.deepEqual(stateEvidence(result.stderr), []);
    }
    for (const args of [["--run"], ["--run", "one", "--run", "two"], ["--describe", "--run", "one"]]) {
      const result = spawnSync(executable, args, { encoding: "utf8", timeout: 5_000 });
      assert.ifError(result.error);
      assert.equal(result.status, 1);
      assert.equal(result.stdout, "");
      assert.deepEqual(stateEvidence(result.stderr), []);
    }
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
