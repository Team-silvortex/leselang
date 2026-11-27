// A fixture-driven interoperability probe using only the Node standard library.
import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createInterface } from "node:readline";
import test from "node:test";
import { fileURLToPath } from "node:url";

const suffix = process.platform === "win32" ? ".exe" : "";
const worker = process.env.LESE_WORKER ?? fileURLToPath(
  new URL(`../../../target/lean/leselang-agent-worker${suffix}`, import.meta.url));
const fixture = JSON.parse(await readFile(process.env.LESE_OPERATIONS_FIXTURE ??
  new URL("../../../fixtures/application-operations/two-effects.json", import.meta.url), "utf8"));
const initialBalance = 9_007_199_254_740_992n;

function runMeter(grants) {
  const scope = { window: "meter", epoch: randomUUID(), session: randomUUID() };
  const version = { agent_worker_version: 4 };
  const challenges = new Set();
  let balance = initialBalance;
  let terminal;
  return new Promise((resolve, reject) => {
    const child = execFile(worker, ["--serve"], {
      timeout: 10_000, killSignal: "SIGKILL", maxBuffer: 64 * 1024, encoding: "utf8",
    }, (error, stdout) => {
      if (error && (error.killed || !Number.isInteger(error.code))) return reject(error);
      try {
        assert.ok(stdout.endsWith("\n") && terminal, "expected a complete terminal record");
        const exitCode = error?.code ?? 0;
        if (terminal.kind === "report") assert.equal(exitCode, 0);
        else assert.notEqual(exitCode, 0);
        resolve({ terminal, calls: challenges.size, balance });
      } catch (failure) { reject(failure); }
    });
    function abort(failure) {
      child.kill("SIGKILL");
      reject(failure);
    }
    child.stdin.on("error", abort);
    const send = (frame) => child.stdin.write(JSON.stringify(frame) + "\n");
    createInterface({ input: child.stdout, crlfDelay: Infinity }).on("line", (line) => {
      try {
        assert.equal(terminal, undefined, "unexpected frame after termination");
        const frame = JSON.parse(line);
        assert.equal(frame.agent_worker_version, 4);
        if (frame.kind === "report" || frame.kind === "error") {
          terminal = frame;
          return;
        }

        const { challenge, request } = frame;
        assert.deepEqual(frame, { ...version, kind: "request", challenge, request });
        assert.equal(typeof challenge, "string");
        assert.match(challenge, /^[a-f0-9]{64}$/u);
        assert.ok(!challenges.has(challenge), "each request needs a fresh challenge");
        const serial = challenges.size + 1;
        const operation = fixture.operations[serial - 1];
        assert.ok(operation && grants.includes(operation.capability), "ungranted or extra Effect");
        const budget = request.budget_ms;
        assert.ok(Number.isInteger(budget) && budget > 0 && budget <= 5000);
        const identity = { transport_version: 1, ...scope, request_id: serial };
        assert.deepEqual(request, { ...identity, budget_ms: budget, operation: operation.name,
          arguments: operation.name === "meter.add" ? { amount: "1" } : {} });
        challenges.add(challenge);
        switch (operation.name) {
          case "meter.add": balance += BigInt(request.arguments.amount); break;
          case "meter.read": break;
          default: throw new Error("Unexpected meter operation");
        }
        send({ ...version, kind: "reply", challenge,
          reply: { ...identity, status: "ok", value: balance.toString(), error: null } });
      } catch (failure) { abort(failure); }
    });
    send({ ...version, kind: "start", profile: "lese.operations", scope, source: fixture.source,
      arguments: fixture.arguments, operations: fixture.operations, grants });
  });
}

test("a host can run the neutral two-Effect fixture against an external worker", async () => {
  const { terminal, balance, calls } = await runMeter(fixture.grants);
  assert.equal(calls, fixture.expected.effects);
  assert.equal(balance.toString(), fixture.expected.result.value);
  assert.deepEqual(terminal, { agent_worker_version: 4, kind: "report", report_json: terminal.report_json });
  assert.deepEqual(JSON.parse(terminal.report_json), {
    schema_version: 1, kind: "operation_task_run", profile: "lese.operations", ...fixture.expected,
  });
});

test("a read-only host receives a denial before either meter callback", async () => {
  const { terminal, balance, calls } = await runMeter(["meter.read"]);
  assert.equal(calls, 0);
  assert.equal(balance, initialBalance);
  assert.deepEqual(terminal, { agent_worker_version: 4, kind: "error", error: terminal.error });
  assert.equal(typeof terminal.error, "string");
  assert.ok(terminal.error.length > 0);
});
