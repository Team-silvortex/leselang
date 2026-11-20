import assert from "node:assert/strict";
import test from "node:test";
import { prepareSemanticCatalog } from "@leselang/bridge";

const field = (name) => ({ kind: "field", name });
const evidencePath = () => [field("observations"), { kind: "match", field: "name", value: "selected" }, field("value")];
const literal = (kind, value) => ({ kind: "literal", value: { kind, value } });
const scalar = (name, scalar_type) => ({ kind: "scalar", path: [field(name)], scalar_type });
const exact = (value) => typeof value === "bigint" ? String(value)
  : Array.isArray(value) ? `[${value.map(exact).join(",")}]`
  : value !== null && typeof value === "object"
    ? `{${Object.entries(value).map(([name, item]) => `${JSON.stringify(name)}:${exact(item)}`).join(",")}}`
    : JSON.stringify(value);

function declaration({ version = 1, observed = false, type = "string", value = "accepted" } = {}) {
  return {
    schema_version: 1, protocol_version: 1, catalog_version: 1, profile: "diagnostics.records",
    operations: [{ name: "records.read", required_capability: "records.read", parameters: [], result: { kind: "scalar", name: type } }],
    native_types: [],
    semantics: { version,
      operations: [{ operation: "records.read", summary: "Read the current record.", effect: "read", completion: "value" }],
      tasks: [{ id: "read_record", summary: "Read and independently verify a record.", inputs: [], execution: "no_text",
        steps: [{ kind: "call", binding: "current", operation: "records.read", arguments: [] }],
        result: { kind: "binding", name: "current" },
        verification: { kind: "record_run", max_fuel: 100, require_verified: true,
          result: observed ? { kind: "observed", scalar_type: type, path: evidencePath() } : { kind: type, value },
          predicates: [
            { id: "fresh_evidence", left: scalar("proof", "boolean"), relation: { kind: "equal" }, right: literal("boolean", true) },
            { id: "stable_metadata", left: { kind: "path", path: [field("before")] }, relation: { kind: "equal" },
              right: { kind: "path", path: [field("after")] } },
          ],
        },
      }],
    },
  };
}
function report(type = "string", value = "accepted") {
  return { schema_version: 1, profile: "diagnostics.records", kind: "record_run", effects: 1, fuel_remaining: 99,
    verified: true, result: { type, value }, observations: [{ name: "selected", value }], proof: true,
    before: { untouched: "private unrelated value" }, after: { untouched: "private unrelated value" } };
}
const prepare = (options) => prepareSemanticCatalog(exact(declaration(options))).prepare("read_record", {});

function rejected(prepared, evidence, code, predicate) {
  const result = prepared.verifyDetailed(evidence);
  const diagnostic = { code, ...(predicate === undefined ? {} : { predicate }) };
  assert.deepEqual(result, { verified: false, diagnostic });
  assert.equal(prepared.verify(evidence), false, "legacy Boolean verification agrees");
  assert.ok(Object.isFrozen(result));
  assert.ok(Object.isFrozen(result.diagnostic));
  assert.throws(() => { result.diagnostic.code = "changed"; }, TypeError);
  return result;
}

test("detailed verification preserves fixed and observed acceptance across semantic versions", () => {
  for (const version of [1, 2, 3]) {
    for (const observed of version === 1 ? [false] : [false, true]) {
      const prepared = prepare({ version, observed });
      for (const evidence of [report(), exact(report())]) {
        const result = prepared.verifyDetailed(evidence);
        assert.deepEqual(result, { verified: true });
        assert.ok(Object.isFrozen(result));
        assert.equal(prepared.verify(evidence), true);
      }
    }
  }
});

test("malformed reports and duplicate keys have bounded value-free diagnostics", () => {
  const prepared = prepare();
  let invoked = 0;
  const accessor = report();
  Object.defineProperty(accessor, "secret", { enumerable: true, get() { invoked++; throw new Error("private exception"); } });
  const toJSON = { ...report(), toJSON() { invoked++; return report(); } };
  const unsafe = report("integer", Number.MAX_SAFE_INTEGER + 1);
  const malformed = [null, [], "", "{", "null", exact(report()) + " private trailing text",
    exact(report()).replace('"proof":true', '"proof":true,"proof":true'),
    exact(report()).replace('"type":"string"', '"type":"string","type":"string"'),
    { ...report(), secret: "x".repeat(4097) }, accessor, toJSON, unsafe];
  for (const evidence of malformed) rejected(prepared, evidence, "invalid_report");
  assert.equal(invoked, 0, "diagnostics never invoke getters or custom serialization");
});

test("identity, budget and required host verification fail in their existing check order", () => {
  const prepared = prepare();
  for (const change of [
    (r) => { delete r.schema_version; }, (r) => { r.schema_version = "1"; }, (r) => { r.schema_version = 2; },
    (r) => { r.profile = "private_foreign_profile"; }, (r) => { r.kind = "private_foreign_kind"; },
  ]) {
    const value = report(); change(value); rejected(prepared, value, "report_identity_mismatch");
  }
  for (const change of [
    (r) => { r.effects = 2; }, (r) => { r.effects = "1"; }, (r) => { delete r.effects; },
    (r) => { r.fuel_remaining = 101; }, (r) => { r.fuel_remaining = -1; }, (r) => { delete r.fuel_remaining; },
  ]) {
    const value = report(); change(value); rejected(prepared, value, "report_budget_mismatch");
  }
  for (const value of [false, "true", 1, null, undefined]) {
    const evidence = report();
    if (value === undefined) delete evidence.verified; else evidence.verified = value;
    rejected(prepared, evidence, "host_verification_missing");
  }
  const value = declaration(); value.semantics.tasks[0].verification.require_verified = false;
  const evidence = report(); delete evidence.verified;
  assert.deepEqual(prepareSemanticCatalog(value).prepare("read_record", {}).verifyDetailed(evidence), { verified: true });
});

test("fixed values and malformed typed results identify result mismatch without exposing values", () => {
  for (const observed of [false, true]) {
    const prepared = prepare({ version: 2, observed });
    for (const change of [
      (r) => { r.result.type = "integer"; }, (r) => { r.result.value = true; },
      (r) => { delete r.result.value; }, (r) => { r.result = null; }, (r) => { delete r.result; },
    ]) {
      const value = report(); change(value); rejected(prepared, value, "result_mismatch");
    }
  }
  const privateValue = 'private text with "quotes" and a newline\n';
  const result = rejected(prepare(), report("string", privateValue), "result_mismatch");
  assert.equal(JSON.stringify(result).includes(privateValue), false);
});

test("observed results distinguish unusable evidence from exact value disagreement", () => {
  const prepared = prepare({ version: 2, observed: true });
  for (const change of [
    (r) => { delete r.observations; }, (r) => { r.observations = []; },
    (r) => { r.observations.push({ ...r.observations[0] }); }, (r) => { r.observations[0].name = "other"; },
    (r) => { delete r.observations[0].value; }, (r) => { r.observations[0].value = 1; },
  ]) {
    const value = report(); change(value); rejected(prepared, value, "result_evidence_invalid");
  }
  const changed = report(); changed.observations[0].value = "private changed observation";
  rejected(prepared, changed, "result_evidence_mismatch");
});

test("predicate diagnostics name only the earliest admitted predicate and distinguish invalid evidence", () => {
  const prepared = prepare();
  const failed = report(); failed.proof = false;
  rejected(prepared, failed, "predicate_failed", "fresh_evidence");
  for (const change of [(r) => { delete r.proof; }, (r) => { r.proof = "true"; }]) {
    const value = report(); change(value); rejected(prepared, value, "predicate_evidence_invalid", "fresh_evidence");
  }
  const changed = report(); changed.after.untouched = "private unrelated mutation";
  rejected(prepared, changed, "predicate_failed", "stable_metadata");
  delete changed.after;
  rejected(prepared, changed, "predicate_evidence_invalid", "stable_metadata");

  const selected = declaration();
  selected.semantics.tasks[0].verification.predicates[0].left = { kind: "path", path: evidencePath() };
  selected.semantics.tasks[0].verification.predicates[0].right = literal("string", "accepted");
  const matched = prepareSemanticCatalog(selected).prepare("read_record", {});
  const ambiguous = report(); ambiguous.observations.push({ ...ambiguous.observations[0] });
  rejected(matched, ambiguous, "predicate_evidence_invalid", "fresh_evidence");
});

test("detailed verification preserves exact u64 results, evidence and predicate arithmetic", () => {
  const maximum = 18_446_744_073_709_551_615n;
  for (const observed of [false, true]) {
    const prepared = prepare({ version: 2, observed, type: "integer", value: maximum });
    const evidence = exact(report("integer", maximum));
    assert.deepEqual(prepared.verifyDetailed(evidence), { verified: true });
    assert.equal(prepared.verify(evidence), true);
    rejected(prepared, JSON.parse(evidence), "invalid_report");
    const changed = evidence.replace('"value":18446744073709551615', '"value":18446744073709551614');
    rejected(prepared, changed, observed ? "result_evidence_mismatch" : "result_mismatch");
  }
  const value = declaration({ type: "integer", value: maximum });
  value.semantics.tasks[0].verification.predicates = [{ id: "counter_bound", left: scalar("count", "integer"),
    relation: { kind: "at_most" }, right: literal("integer", maximum) }];
  const bounded = prepareSemanticCatalog(exact(value)).prepare("read_record", {});
  const evidence = { ...report("integer", maximum), count: maximum };
  assert.deepEqual(bounded.verifyDetailed(exact(evidence)), { verified: true });
  rejected(bounded, exact({ ...evidence, count: "private invalid counter" }), "predicate_evidence_invalid", "counter_bound");
  value.semantics.tasks[0].verification.predicates[0].relation = { kind: "increased_by", amount: 1 };
  const overflow = prepareSemanticCatalog(exact(value)).prepare("read_record", {});
  rejected(overflow, exact(evidence), "predicate_failed", "counter_bound");
});

test("several failures preserve the original verification order", () => {
  const fixed = prepare();
  let evidence = { ...report(), profile: "wrong", effects: 2, verified: false, result: null, proof: false };
  rejected(fixed, { ...evidence, secret: "x".repeat(4097) }, "invalid_report");
  rejected(fixed, evidence, "report_identity_mismatch");
  evidence.profile = "diagnostics.records";
  rejected(fixed, evidence, "report_budget_mismatch");
  evidence.effects = 1;
  rejected(fixed, evidence, "host_verification_missing");
  evidence.verified = true;
  rejected(fixed, evidence, "result_mismatch");
  evidence.result = { type: "string", value: "accepted" };
  delete evidence.after;
  rejected(fixed, evidence, "predicate_failed", "fresh_evidence");

  const observed = prepare({ version: 3, observed: true });
  evidence = report(); evidence.proof = false; delete evidence.observations;
  rejected(observed, evidence, "result_evidence_invalid");
  evidence.observations = [{ name: "selected", value: "different" }];
  rejected(observed, evidence, "result_evidence_mismatch");
});
