import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { AgentSession } from "@leselang/bridge";
import { createNodeOperationHost } from "@leselang/bridge/node";
import { createOperationRegistry } from "@leselang/bridge/operations";
import { worker } from "./operations.mjs";

function createMeter(initialBalance, writeReply) {
  assert.ok(["balance", "string"].includes(writeReply), `Unknown meter reply mode: ${writeReply}`);
  const state = { balance: initialBalance, addCalls: 0, readCalls: 0, committedWrites: 0 };
  const registry = createOperationRegistry([
    { name: "meter.add", parameters: { amount: "integer" }, result: "integer", capability: "meter.write",
      handle({ amount }, context) {
        state.addCalls++;
        context.commit(() => { state.balance += amount; state.committedWrites++; });
        return writeReply === "string" ? "wrong type" : state.balance;
      } },
    { name: "meter.read", parameters: {}, result: "integer", capability: "meter.read",
      handle() { state.readCalls++; return state.balance; } },
  ]);
  return { registry, state };
}

test("the Node meter matches the Rust and .NET operation fixture", async (t) => {
  const fixture = JSON.parse(await readFile(new URL("../../../fixtures/application-operations/two-effects.json", import.meta.url), "utf8"));
  const { registry, state } = createMeter(9_007_199_254_740_992n, "balance");
  const host = createNodeOperationHost({ executable: worker, registry, profile: "lese.operations",
    grants: () => fixture.grants, observe: () => state.balance.toString(),
    verify: ({ report, before, after }) => BigInt(after) === BigInt(before) + 1n && report.result.value === after,
  });
  t.after(() => host.close());
  const session = new AgentSession({ host, provider: {
    async plan({ catalog }) {
      assert.equal(catalog.profile, "lese.operations");
      assert.deepEqual(JSON.parse(JSON.stringify(catalog.operations)), fixture.operations);
      return { source: fixture.source, arguments: fixture.arguments, summary: "Increment and read the meter" };
    },
  } });
  const outcome = await session.submit("increment and read");
  assert.equal(outcome.status, "verified", outcome.error);
  const { profile, effects, result, fuel_remaining } = JSON.parse(outcome.report);
  assert.equal(profile, "lese.operations");
  assert.deepEqual({ effects, result, fuel_remaining }, fixture.expected);
});

test("the Node meter matches the shared operation failure vectors", async (t) => {
  const fixture = JSON.parse(await readFile(new URL("../../../fixtures/application-operations/failure-vectors.json", import.meta.url), "utf8"));
  assert.equal(fixture.entry, "main");
  assert.deepEqual(fixture.cases.map(({ id }) => id).sort(), ["missing-grant", "post-write-invalid-result"]);

  for (const sample of fixture.cases) {
    await t.test(sample.id, async (t) => {
      const { registry, state } = createMeter(BigInt(fixture.initial_balance), sample.write_reply);
      const host = createNodeOperationHost({
        executable: worker, registry, profile: "lese.operations",
        grants: () => sample.grants,
        verify: () => true,
      });
      t.after(() => host.close());
      const session = new AgentSession({ host, provider: {
        async plan({ catalog }) {
          assert.equal(catalog.profile, "lese.operations");
          return { source: fixture.source, arguments: {}, summary: "Increment and read the meter" };
        },
      } });
      const outcome = await session.submit("increment and read");
      assert.equal(outcome.plan?.source, fixture.source);
      assert.equal(outcome.possiblePartialEffects, true); // The session entered host execution.
      assert.equal(outcome.report, undefined);
      const observed = {
        outcome: outcome.status,
        add_calls: state.addCalls,
        read_calls: state.readCalls,
        committed_writes: state.committedWrites,
        balance: state.balance.toString(),
      };
      assert.deepEqual(observed, sample.expected);
      t.diagnostic(`Shared failure ${sample.id}: ${JSON.stringify(observed)}`);
    });
  }
});

test("only an exact filter returned by the application is selected", async (t) => {
  const fixture = JSON.parse(await readFile(new URL("../../../fixtures/application-operations/list-membership.json", import.meta.url), "utf8"));
  for (const { options, wanted, matched } of fixture.cases) {
    let reads = 0;
    const selections = [];
    const registry = createOperationRegistry([
      { name: "filters.available", parameters: {}, result: "string_list", capability: "filters.read",
        handle() { reads++; return options; } },
      { name: "filters.select", parameters: { value: "string" }, result: "boolean", capability: "filters.write",
        handle({ value }, context) { context.commit(() => selections.push(value)); return true; } },
    ]);
    const host = createNodeOperationHost({ executable: worker, registry, profile: "lese.operations",
      grants: () => ["filters.read", "filters.write"],
      verify: ({ report }) => report.result.type === "boolean" && report.result.value === matched
        && report.effects === (matched ? 2 : 1) && reads === 1
        && (matched ? selections.length === 1 && selections[0] === wanted : selections.length === 0),
    });
    t.after(() => host.close());
    const session = new AgentSession({ host, provider: { async plan() {
      return { source: fixture.source, arguments: { wanted }, summary: "Select an available filter" };
    } } });
    const outcome = await session.submit("select filter");
    assert.equal(outcome.status, "verified", outcome.error);
  }
});
