import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { AgentSession } from "@leselang/bridge";
import { createNodeOperationHost } from "@leselang/bridge/node";
import { createOperationRegistry } from "@leselang/bridge/operations";
import { worker } from "./operations.mjs";

const fixtureUrl = new URL("../../../fixtures/application-operations/lost-reply.json", import.meta.url);
const suffix = process.platform === "win32" ? ".exe" : "";
const relay = fileURLToPath(new URL(`../../../target/lean/examples/lost_reply_relay${suffix}`, import.meta.url));
const workerExecutable = worker + suffix;
const writeGrants = () => ["ledger.write", "ledger.read"];

function createLedger(initialBalance, operation) {
  const state = { balance: BigInt(initialBalance), addCalls: 0, writes: 0, tailReads: 0, lookupKeys: [] };
  const receipts = new Map();
  const registry = createOperationRegistry([
    { name: operation, parameters: { key: "string" }, result: "integer", capability: "ledger.write",
      handle({ key }, context) {
        state.addCalls++;
        if (operation === "ledger.add_once" && receipts.has(key)) return receipts.get(key);
        context.commit(() => {
          state.balance++;
          state.writes++;
          receipts.set(key, state.balance);
        });
        return state.balance;
      } },
    { name: "ledger.read", parameters: {}, result: "integer", capability: "ledger.read",
      handle() { state.tailReads++; return state.balance; } },
    { name: "ledger.lookup", parameters: { key: "string" }, result: "optional_string", capability: "ledger.lookup",
      handle({ key }) { state.lookupKeys.push(key); return receipts.get(key)?.toString() ?? null; } },
  ]);
  return { state, registry };
}

async function checkInterruptedWrite({ registry, state }, fixture, source, sample) {
  let diagnostics = "";
  const failed = await submit({
    executable: relay, registry, grants: writeGrants, verify: () => false,
    onStderr(chunk) { diagnostics += chunk; },
  }, source, sample.key, "Interrupt the ledger write");
  assert.equal(failed.plan?.source, source);
  assert.equal(failed.status, "failed", failed.error);
  assert.equal(failed.possiblePartialEffects, true);
  assert.equal(failed.report, undefined);
  assert.ok(diagnostics.split(/\r?\n/u).includes(`lese-test-cut ${sample.id}`), diagnostics);
  const receipt = await lookup(registry, fixture.lookup_source, sample.key, sample.expected.lookup_receipt);
  const unknownReceipt = await lookup(registry, fixture.lookup_source, fixture.unknown_key, sample.expected.unknown_receipt);
  assert.deepEqual(state.lookupKeys, [sample.key, fixture.unknown_key]);
  const observed = {
    run_report: failed.report !== undefined,
    add_calls: state.addCalls,
    committed_writes: state.writes,
    tail_reads: state.tailReads,
    lookup_receipt: receipt,
    unknown_receipt: unknownReceipt,
    balance: state.balance.toString(),
  };
  assert.deepEqual(observed, sample.expected);
  return observed;
}

async function lookup(registry, source, key, expected) {
  const recovered = await submit({
    executable: workerExecutable, registry, grants: () => ["ledger.lookup"],
    verify: ({ report }) => report.effects === 1
      && report.result.type === "optional_string" && report.result.value === expected,
  }, source, key, "Read the application receipt");
  assert.equal(recovered.status, "verified", recovered.error);
  return JSON.parse(recovered.report).result.value;
}

// Each submission owns a fresh carrier, worker and AgentSession. Only the application's ledger survives.
async function submit(options, source, key, summary) {
  const host = createNodeOperationHost({ profile: "lese.operations", ...options });
  try {
    const session = new AgentSession({ host, provider: { async plan({ catalog }) {
      assert.equal(catalog.profile, "lese.operations");
      return { source, arguments: { key }, summary };
    } } });
    return await session.submit(summary);
  } finally { host.close(); }
}

test("Node reconciles a lost worker reply with a read-only receipt lookup", async (t) => {
  const fixture = JSON.parse(await readFile(fixtureUrl, "utf8"));
  assert.deepEqual(fixture.cases.map(({ id }) => id).sort(), ["reply-lost", "request-lost"]);
  assert.equal(fixture.replay_control.case, "reply-lost");
  for (const sample of fixture.cases) {
    await t.test(sample.id, async (t) => {
      const ledger = createLedger(fixture.initial_balance, "ledger.add");
      const { state, registry } = ledger;
      const observed = await checkInterruptedWrite(ledger, fixture, fixture.write_source, sample);
      t.diagnostic(`${sample.id} ${JSON.stringify(observed)}`);

      if (sample.id === fixture.replay_control.case) {
        const replay = await submit({
          executable: workerExecutable, registry, grants: writeGrants,
          verify: ({ report }) => report.effects === 2 && report.result.type === "integer"
            && report.result.value === state.balance.toString(),
        }, fixture.write_source, sample.key, "Deliberately repeat the original write");
        assert.equal(replay.status, "verified", replay.error);
        assert.equal(state.addCalls, observed.add_calls + 1);
        assert.equal(state.tailReads, observed.tail_reads + 1);
        const control = { case: sample.id, committed_writes: state.writes, balance: state.balance.toString() };
        assert.deepEqual(control, fixture.replay_control);
        t.diagnostic(`blind replay ${JSON.stringify(control)}`);
      }
    });
  }
});

test("Node retries a settled write once per business key after either cut", async (t) => {
  const fixture = JSON.parse(await readFile(fixtureUrl, "utf8"));
  assert.deepEqual(fixture.cases.map(({ id }) => id).sort(), ["reply-lost", "request-lost"]);
  for (const sample of fixture.cases) {
    await t.test(sample.id, async (t) => {
      const ledger = createLedger(fixture.initial_balance, "ledger.add_once");
      const { state, registry } = ledger;
      await checkInterruptedWrite(ledger, fixture, fixture.idempotent_source, sample);

      async function write(key, expectedReceipt) {
        const outcome = await submit({
          executable: workerExecutable, registry, grants: writeGrants,
          verify: ({ report }) => report.effects === 2 && report.result.type === "integer"
            && report.result.value === expectedReceipt,
        }, fixture.idempotent_source, key, "Increment once using the supplied operation key");
        assert.equal(outcome.status, "verified", outcome.error);
        return JSON.parse(outcome.report);
      }
      const report = await write(sample.key, sample.idempotent_retry.receipt);
      const observed = {
        add_calls: state.addCalls,
        committed_writes: state.writes,
        tail_reads: state.tailReads,
        effects: report.effects,
        receipt: report.result.value,
        balance: state.balance.toString(),
      };
      assert.deepEqual(observed, sample.idempotent_retry);
      await lookup(registry, fixture.lookup_source, sample.key, observed.receipt);
      t.diagnostic(`idempotent retry ${sample.id} ${JSON.stringify(observed)}`);

      const isolation = fixture.key_isolation;
      assert.notEqual(isolation.other_key, sample.key);
      const other = await write(isolation.other_key, isolation.other_receipt);
      const original = await write(sample.key, isolation.replayed_receipt);
      const isolated = {
        other_key: isolation.other_key,
        other_receipt: other.result.value,
        replayed_receipt: original.result.value,
        balance: state.balance.toString(),
        committed_writes: state.writes,
      };
      assert.deepEqual(isolated, isolation);
      t.diagnostic(`key isolation ${sample.id} ${JSON.stringify(isolated)}`);
    });
  }
});
