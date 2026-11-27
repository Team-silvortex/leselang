import assert from "node:assert/strict";
import test from "node:test";
import { createOperationRegistry, defineOperation, readOperationValue } from "../dist/operations/index.js";

const grant = (allows = () => true) => ({ signal: new AbortController().signal, allows });
const definition = (overrides = {}) => defineOperation({
  name: "batch.reserve", parameters: { quantity: "integer" }, result: "integer", capability: "batch.write",
  handle: ({ quantity }) => quantity, ...overrides,
});

test("discovery owns public schemas independently of grants and handler identities", async () => {
  let called = 0;
  const def = definition({ handle({ quantity }) { called++; return quantity; } });
  const registry = createOperationRegistry([def]);
  def.capability = "other.write"; def.parameters.quantity = "string"; def.handle = () => assert.fail();
  const [schema] = registry.describe();
  assert.equal(schema.capability, "batch.write");
  assert.equal(schema.parameters.quantity, "integer");
  assert.equal(Object.hasOwn(schema, "handle"), false);
  assert.ok([schema, schema.parameters, registry.describe()].every(Object.isFrozen));
  await assert.rejects(registry.dispatch("batch.reserve", { quantity: "7" }, grant(() => false)), /permission_denied/);
  assert.equal(called, 0);
  const huge = "18446744073709551615";
  assert.equal(await registry.dispatch("batch.reserve", { quantity: huge }, grant((cap) => cap === "batch.write")), huge);
  assert.equal(called, 1);
});

test("schema admission rejects unreadable declarations, duplicates, reserved names and excessive arity", () => {
  let reads = 0;
  const getter = { get quantity() { reads++; return "integer"; } };
  const fnGetter = Object.defineProperty(definition(), "handle", { get() { reads++; } });
  for (const declarations of [[], [definition(), definition()], [fnGetter], [definition({ parameters: getter })],
    [definition({ parameters: Object.fromEntries(Array.from({ length: 9 }, (_, i) => ["a" + i, "boolean"])) })],
    [definition({ name: "batch" })], [definition({ capability: "batch._lese_write" })],
    [definition({ parameters: { body: "string" } })], [definition({ result: "auto" })],
    Array.from({ length: 33 }, (_, i) => definition({ name: "task.op" + i }))]) {
    assert.throws(() => createOperationRegistry(declarations));
  }
  assert.equal(reads, 0);
});

test("invalid wire scalar never reaches handler, async results must match the declared type", async () => {
  let calls = 0, reads = 0;
  const registry = createOperationRegistry([definition({ async handle() { calls++; return 1; } })]);
  for (const input of [{}, { quantity: "1", extra: false }, { get quantity() { reads++; } },
    ...[1, 1n, "-1", "1.0", "01", "18446744073709551616"].map((quantity) => ({ quantity }))]) {
    await assert.rejects(registry.dispatch("batch.reserve", input, grant()));
  }
  await assert.rejects(registry.dispatch("batch.cancel", { quantity: "1" }, grant()), /unknown_operation/);
  assert.equal(calls, 0);
  assert.equal(reads, 0);
  await assert.rejects(registry.dispatch("batch.reserve", { quantity: "1" }, grant()), /invalid_operation_integer/);
  assert.equal(calls, 1);
});

test("strings, optional values and lists retain types and UTF-8 limits without invoking getters", async () => {
  const words = ["one", "二"];
  let reads = 0;
  const a = [];
  Object.defineProperty(a, 0, { enumerable: true, get() { reads++; } });
  const thenable = { get then() { reads++; } };
  assert.equal(readOperationValue("none", null), null);
  assert.equal(readOperationValue("optional_string", null), null);
  assert.equal(readOperationValue("optional_string", ""), "");
  assert.equal(readOperationValue("string", "x".repeat(4096)).length, 4096);
  assert.equal(readOperationValue("boolean", false), false);
  assert.throws(() => readOperationValue("string", "\ud800"));
  assert.throws(() => readOperationValue("string_list", ["文".repeat(1000), "文".repeat(1000)]));
  assert.throws(() => readOperationValue("string_list", new Array(65).fill("")));
  assert.throws(() => readOperationValue("string_list", a));
  const copied = readOperationValue("string_list", words);
  words[0] = "replaced";
  assert.ok(Object.isFrozen(copied));
  assert.deepEqual(copied, ["one", "二"]);
  const registry = createOperationRegistry([definition({ result: "string", handle: () => thenable })]);
  await assert.rejects(registry.dispatch("batch.reserve", { quantity: "1" }, grant()));
  assert.equal(reads, 0);
});

for (const retire of ["deny", "abort"]) {
  test(`a handler held across an await cannot commit after ${retire}`, async () => {
    const controller = new AbortController();
    let allowed = true, writes = 0, wake;
    const held = new Promise((resolve) => { wake = resolve; });
    const registry = createOperationRegistry([definition({ async handle({ quantity }, ctx) {
      ctx.commit(() => writes++);
      await held;
      return ctx.commit(() => { writes++; return quantity; });
    } })]);
    const result = registry.dispatch("batch.reserve", { quantity: "7" }, { signal: controller.signal, allows: () => allowed });
    assert.equal(writes, 1);
    if (retire === "deny") allowed = false;
    else controller.abort();
    wake();
    await assert.rejects(result, /permission_denied|scope_closed/);
    assert.equal(writes, 1);
  });
}

test("settled handlers lose commit authority, including microtasks queued by synchronous handlers", async () => {
  let writes = 0, check;
  const registry = createOperationRegistry([definition({ handle({ quantity }, ctx) {
    ctx.commit(() => writes++);
    check = Promise.resolve().then(() => assert.throws(() => ctx.commit(() => writes++), /scope_closed/));
    return quantity;
  } })]);
  assert.equal(await registry.dispatch("batch.reserve", { quantity: "1" }, grant()), "1");
  await check;
  assert.equal(writes, 1);
});

test("malformed authority never invokes a handler and policy exceptions retire an in-flight dispatch", async () => {
  let checks = 0, calls = 0, writes = 0;
  const registry = createOperationRegistry([definition({ handle(args, context) {
    calls++;
    assert.throws(() => context.commit(() => writes++), /policy unavailable/);
    assert.throws(() => context.commit(() => writes++), /scope_closed/);
    return args.quantity;
  } })]);
  for (const authority of [undefined, { allows: () => true }, { signal: {}, allows: () => true }, { signal: grant().signal }]) {
    await assert.rejects(registry.dispatch("batch.reserve", { quantity: "1" }, authority), /invalid_operation_authority/);
  }
  assert.equal(calls, 0);
  await assert.rejects(registry.dispatch("batch.reserve", { quantity: "1" }, grant(() => {
    if (++checks === 2) throw new Error("policy unavailable");
    return true;
  })), /scope_closed/);
  assert.equal(calls, 1);
  assert.equal(writes, 0);
});
