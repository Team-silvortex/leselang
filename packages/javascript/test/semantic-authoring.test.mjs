import assert from "node:assert/strict";
import test from "node:test";
import { defineSemanticCatalog, prepareSemanticCatalog } from "@leselang/bridge";

const limit = 256 * 1024;
const max = 18_446_744_073_709_551_615n;
const literal = (kind, value) => ({ kind: "literal", value: { kind, value } });
const field = (name) => ({ kind: "field", name });

function base() {
  return {
    schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "author.counter",
    native_types: [],
    operations: [{ name: "counter.read", required_capability: "counter.read", parameters: [],
      result: { kind: "scalar", name: "integer" } }],
  };
}

function semantics(version = 1) {
  return {
    version,
    operations: [{ operation: "counter.read", summary: "Read the current scoped counter.", effect: "read", completion: "value" }],
    tasks: [{
      id: "read_counter", summary: "Read and verify the application counter.", inputs: [], execution: "no_text",
      steps: [{ kind: "call", binding: "count", operation: "counter.read", arguments: [] }],
      result: { kind: "binding", name: "count" },
      verification: {
        kind: "counter_run", max_fuel: 100, require_verified: false,
        result: version === 1 ? { kind: "integer", value: 42 }
          : { kind: "observed", scalar_type: "integer", path: [field("observed_count")] },
        predicates: [{ id: "independent_observation", relation: { kind: "equal" },
          left: { kind: "scalar", scalar_type: "boolean", path: [field("fresh")] },
          right: literal("boolean", true) }],
      },
    }],
  };
}

const report = (integer = "42") => `{"schema_version":1,"profile":"author.counter","kind":"counter_run","result":{"type":"integer","value":${integer}},"effects":1,"fuel_remaining":90,"fresh":true,"observed_count":${integer}}`;

test("host-authored v1 and v2 declarations use the existing planner and independent verifier", () => {
  for (const version of [1, 2]) {
    const raw = defineSemanticCatalog(JSON.stringify(base()), semantics(version));
    assert.equal(typeof raw, "string");
    const catalog = prepareSemanticCatalog(raw);
    assert.equal(catalog.profile, "author.counter");
    assert.deepEqual(catalog.tasks, [{ id: "read_counter", summary: "Read and verify the application counter." }]);
    const prepared = catalog.prepare("read_counter", {});
    assert.equal(prepared.expectedEffects, 1);
    assert.deepEqual(prepared.checks, ["independent_observation"]);
    assert.equal(prepared.verify(report()), true);
    assert.equal(prepared.verify(report().replace('"fresh":true', '"fresh":false')), false);
    assert.equal(prepared.verify(report("43")), version === 2);
    if (version === 2) {
      assert.equal(prepared.verify(report("43").replace('"observed_count":43', '"observed_count":42')), false);
    }
  }
});

test("authoring owns declarations without changing the host base or caller's objects", () => {
  const native = base(), authored = semantics(2);
  const original = structuredClone({ native, authored });
  const raw = defineSemanticCatalog(native, authored);
  assert.deepEqual({ native, authored }, original);
  assert.equal(Object.hasOwn(native, "semantics"), false);
  const before = prepareSemanticCatalog(raw).prepare("read_counter", {}).plan;
  native.profile = "different.host";
  native.operations[0].name = "different.operation";
  authored.tasks[0].summary = "different instructions";
  authored.tasks[0].steps.length = 0;
  const after = prepareSemanticCatalog(raw).prepare("read_counter", {}).plan;
  assert.deepEqual(after, before);
  assert.equal(prepareSemanticCatalog(raw).profile, "author.counter");
});

test("an existing semantic catalog cannot be silently replaced, even by the same declaration", () => {
  for (const existing of [null, semantics(), {}, { version: 99 }]) {
    const native = { ...base(), semantics: existing };
    assert.throws(() => defineSemanticCatalog(native, semantics(2)));
    assert.throws(() => defineSemanticCatalog(JSON.stringify(native), semantics(2)));
  }
});

test("duplicate raw base keys and rounded object integers fail before a catalog is emitted", () => {
  const raw = JSON.stringify(base());
  for (const invalid of [
    raw.replace('"schema_version":1', '"schema_version":1,"schema_version":1'),
    raw.replace('"name":"counter.read"', '"name":"counter.read","name":"counter.read"'),
    raw.slice(0, -1) + ',"metadata":{"x":1,"x":2}}',
    raw + " trailing",
  ]) assert.throws(() => defineSemanticCatalog(invalid, semantics()));
  assert.throws(() => defineSemanticCatalog({ ...base(), metadata: Number.MAX_SAFE_INTEGER + 1 }, semantics()));
  assert.throws(() => defineSemanticCatalog({ ...base(), catalog_version: 1n }, semantics()),
    "the original base-object decoder still requires JSON Number integers");
});

test("raw floating version tokens cannot be normalized into valid integer versions", () => {
  const raw = JSON.stringify(base());
  for (const [name, token] of [["schema_version", "1.0"], ["protocol_version", "1e0"],
    ["catalog_version", "1.0"], ["catalog_version", "-0"]]) {
    assert.throws(() => defineSemanticCatalog(raw.replace(`"${name}":1`, `"${name}":${token}`), semantics()));
  }
});

test("authoring rejects invalid operation references, task types and forward bindings", () => {
  const changes = [
    (value) => { value.operations[0].operation = "counter.missing"; },
    (value) => { value.tasks[0].steps[0].operation = "counter.missing"; },
    (value) => { value.tasks[0].result = { kind: "binding", name: "future" }; },
    (value) => { value.tasks[0].steps.unshift({ kind: "guard", conditions: [{ kind: "equal", left: { kind: "binding", name: "count" }, right: literal("integer", 42) }], failure: { kind: "integer", value: 0 } }); },
    (value) => { value.tasks[0].result = literal("string", "wrong type"); },
    (value) => { value.tasks[0].steps[0].arguments = [{ name: "unknown", value: literal("integer", 42) }]; },
    (value) => { value.tasks[0].verification.result = { kind: "observed", scalar_type: "integer", path: [field("observed_count")] }; },
    (value) => { value.tasks[0].verification.extra_claim = true; },
    (value) => { value.tasks[0].inputs = [{ name: "text", scalar_type: "string" }]; },
  ];
  for (const change of changes) {
    const value = semantics(); change(value);
    assert.throws(() => defineSemanticCatalog(base(), value));
  }
  const observed = semantics(2);
  observed.tasks[0].verification.result.path = [field("result"), field("value")];
  assert.throws(() => defineSemanticCatalog(base(), observed), "observed results need independent evidence");
});

test("accessors and toJSON hooks are never executed while owning authoring inputs", () => {
  let calls = 0;
  const getter = { enumerable: true, get() { calls++; return "not inspected"; } };
  const native = base(); Object.defineProperty(native, "profile", getter);
  assert.throws(() => defineSemanticCatalog(native, semantics()));
  const declaration = semantics(); Object.defineProperty(declaration.tasks[0], "summary", getter);
  assert.throws(() => defineSemanticCatalog(base(), declaration));
  const nativeHook = base(); nativeHook.toJSON = () => { calls++; return {}; };
  assert.throws(() => defineSemanticCatalog(nativeHook, semantics()));
  const authoredHook = semantics(); authoredHook.toJSON = () => { calls++; return {}; };
  assert.throws(() => defineSemanticCatalog(base(), authoredHook));
  assert.equal(calls, 0);
});

test("cycles, array holes, symbol properties and non-JSON prototypes are refused", () => {
  const cycle = semantics(); cycle.tasks[0].verification.predicates.push(cycle);
  assert.throws(() => defineSemanticCatalog(base(), cycle));
  const hole = semantics(); delete hole.tasks[0].steps[0];
  assert.throws(() => defineSemanticCatalog(base(), hole));
  const symbol = semantics(); symbol[Symbol("hidden")] = true;
  assert.throws(() => defineSemanticCatalog(base(), symbol));
  const prototype = Object.assign(Object.create({ inherited: true }), semantics());
  assert.throws(() => defineSemanticCatalog(base(), prototype));
  assert.throws(() => defineSemanticCatalog(new Date(), semantics()));
  const nonEnumerable = semantics(); Object.defineProperty(nonEnumerable, "hidden", { value: true });
  assert.throws(() => defineSemanticCatalog(base(), nonEnumerable));
});

test("the complete emitted catalog obeys the byte bound after combining valid inputs", () => {
  const native = { ...base(), extension: "x".repeat(limit - 1024) };
  const authored = semantics();
  authored.operations[0].summary = "o".repeat(1024);
  authored.tasks[0].summary = "t".repeat(1024);
  assert.ok(Buffer.byteLength(JSON.stringify(native)) < limit);
  assert.ok(Buffer.byteLength(JSON.stringify(authored)) < limit);
  assert.ok(Buffer.byteLength(JSON.stringify({ ...native, semantics: authored })) > limit);
  assert.throws(() => defineSemanticCatalog(native, authored));
});

test("combining individually bounded inputs cannot exceed the complete catalog's node budget", () => {
  const native = { ...base(), extension: Array(65_400).fill(0) };
  const authored = semantics();
  const predicate = authored.tasks[0].verification.predicates[0];
  authored.tasks[0].verification.predicates = Array.from({ length: 64 }, (_, index) => ({
    ...structuredClone(predicate), id: `observation_${index}`,
  }));
  assert.doesNotThrow(() => defineSemanticCatalog(native, semantics()), "the larger base is valid alone with a small declaration");
  assert.doesNotThrow(() => defineSemanticCatalog(base(), authored), "all 64 declared checks are independently valid");
  assert.ok(Buffer.byteLength(JSON.stringify({ ...native, semantics: authored })) < limit,
    "the failure is not the serialized byte ceiling");
  assert.throws(() => defineSemanticCatalog(native, authored));
});

test("depth, node and Unicode bounds apply to both authored data and the supplied base", () => {
  const nested = (count) => {
    let value = null;
    for (let index = 0; index < count; index++) value = { next: value };
    return value;
  };
  assert.doesNotThrow(() => defineSemanticCatalog({ ...base(), extension: nested(31) }, semantics()));
  assert.throws(() => defineSemanticCatalog({ ...base(), extension: nested(32) }, semantics()));
  assert.throws(() => defineSemanticCatalog({ ...base(), extension: Array(65_536).fill(null) }, semantics()));
  for (const value of ["\ud800", "\udfff", "😀".repeat(257)]) {
    const authored = semantics(); authored.tasks[0].summary = value;
    assert.throws(() => defineSemanticCatalog(base(), authored));
  }
});

test("u64 authoring emits exact integer tokens and preserves existing reader ownership rules", () => {
  const authored = semantics();
  authored.tasks[0].result = literal("integer", max);
  authored.tasks[0].verification.result = { kind: "integer", value: max };
  authored.tasks[0].verification.max_fuel = 100n;
  authored.tasks[0].verification.predicates.push({ id: "exact_count", relation: { kind: "equal" },
    left: { kind: "scalar", scalar_type: "integer", path: [field("observed_count")] },
    right: literal("integer", max) });
  const rawBase = JSON.stringify(base()).slice(0, -1) + ',"metadata":{"exact":18446744073709551615}}';
  const raw = defineSemanticCatalog(rawBase, authored);
  assert.match(raw, /"exact":18446744073709551615/u);
  assert.match(raw, /"value":18446744073709551615/u);
  assert.equal(raw.includes('"18446744073709551615"'), false);
  const prepared = prepareSemanticCatalog(raw).prepare("read_counter", {});
  assert.match(prepared.plan.source, /18446744073709551615/u);
  assert.equal(prepared.verify(report(String(max))), true);
  assert.equal(prepared.verify(report(String(max - 1n))), false);
  assert.throws(() => prepareSemanticCatalog({ ...base(), semantics: authored }),
    "the ordinary object reader does not gain implicit BigInt support");
  assert.throws(() => prepareSemanticCatalog(JSON.parse(raw)), "rounding is not repaired after JSON.parse");
});

test("authoring never recovers an unsigned integer from a lossy or invalid numeric value", () => {
  for (const value of [Number.MAX_SAFE_INTEGER + 1, NaN, Infinity, -1, 0.5, -1n, max + 1n]) {
    const authored = semantics();
    authored.tasks[0].result = literal("integer", value);
    authored.tasks[0].verification.result = { kind: "integer", value };
    assert.throws(() => defineSemanticCatalog(base(), authored));
  }
  const authored = semantics();
  authored.tasks[0].verification.max_fuel = max;
  assert.throws(() => defineSemanticCatalog(base(), authored), "Fuel retains its existing smaller bound");
});
