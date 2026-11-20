/** Check fresh host evidence against the task's admitted report contract. */
import { verificationResultType, type Catalog } from "./catalog.js";
import { bool, decode, encoder, fail, list, record, require_, text, uint, U64_MAX, type Json, type RecordValue, type ScalarType } from "./json.js";
import { rejectVerification, VERIFICATION_SUCCESS, type VerificationFailureCode, type VerificationResult } from "../verification.js";

function scalar(value: Json | undefined, type: ScalarType): Json {
  if (value === undefined) fail("missing report scalar");
  switch (type) {
    case "integer": uint(value); break;
    case "boolean": bool(value); break;
    case "string": text(value, 4096); break;
    case "none": require_(value === null, "report none"); break;
    case "optional_string": if (value !== null) text(value, 4096); break;
    case "string_list": {
      const values = list(value, 64); let length = 0;
      for (const value of values) length += encoder.encode(text(value, 4096)).length;
      require_(length <= 4096, "string list bound"); break;
    }
  }
  return value;
}
function literalJson(value: Json | undefined): Json {
  const data = record(value); return data.kind === "none" ? null : data.value!;
}
function equal(left: Json, right: Json): boolean {
  if (typeof left !== typeof right) return false;
  if (left === null || right === null || typeof left !== "object" || typeof right !== "object") return left === right;
  if (Array.isArray(left) || Array.isArray(right)) return Array.isArray(left) && Array.isArray(right)
    && left.length === right.length && left.every((value, index) => equal(value, right[index]!));
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every((key) => Object.hasOwn(right, key) && equal(left[key]!, right[key]!));
}
function path(report: Json, segments: Json[]): Json {
  let value = report;
  for (const raw of segments) {
    const segment = record(raw);
    if (segment.kind === "field") {
      const fields = record(value);
      if (!Object.hasOwn(fields, segment.name as string)) fail("missing report field");
      value = fields[segment.name as string]!;
    } else {
      const matches = list(value, 65_536).filter((item) => item !== null && typeof item === "object" && !Array.isArray(item)
        && item[segment.field as string] === segment.value);
      if (matches.length !== 1) fail("missing or ambiguous report selection");
      value = matches[0]!;
    }
  }
  return value;
}
function reportTerm(report: Json, value: Json | undefined, args: Readonly<Record<string, string>>): Json {
  const term = record(value);
  if (term.kind === "literal") return literalJson(term.value);
  if (term.kind === "input") return args[term.name as string]!;
  const selected = path(report, list(term.path, 8));
  if (term.kind === "length") return BigInt(list(selected, 65_536).length);
  if (term.kind === "scalar") return scalar(selected, term.scalar_type as ScalarType);
  return selected;
}
function reportBounds(value: Json): void {
  if (typeof value === "string") text(value, 4096);
  else if (Array.isArray(value)) value.forEach(reportBounds);
  else if (value !== null && typeof value === "object") {
    for (const [key, entry] of Object.entries(value)) { text(key, 4096); reportBounds(entry); }
  }
}
const executionCodes = new Set<VerificationFailureCode>([
  "execution_failed", "execution_object_unavailable", "execution_fuel_exhausted",
  "execution_deadline_exceeded", "execution_scope_closed", "execution_cancelled",
]);

/** Only an explicitly selected host contract gives these otherwise ordinary fields meaning. */
function executionFailure(report: RecordValue): VerificationResult | undefined {
  const hasStatus = Object.hasOwn(report, "status"), hasFailure = Object.hasOwn(report, "execution_failure");
  if (!hasStatus && !hasFailure) return undefined;
  try {
    require_(report.status === "failed" || report.status === "cancelled", "execution report status");
    text(report.error, 4096, true);
    bool(report.possible_partial_effects);
    let code: VerificationFailureCode = report.status === "cancelled" ? "execution_cancelled" : "execution_failed";
    if (hasFailure) {
      const marker = record(report.execution_failure, ["code"]);
      require_(typeof marker.code === "string" && executionCodes.has(marker.code as VerificationFailureCode), "execution failure code");
      code = marker.code as VerificationFailureCode;
      require_((report.status === "cancelled") === (code === "execution_cancelled"), "execution failure status mismatch");
    }
    return rejectVerification(code);
  } catch { return rejectVerification("invalid_report"); }
}

export function verifyDetailed(catalog: Catalog, task: RecordValue, args: Readonly<Record<string, string>>, effects: number, raw: unknown,
  executionReports?: "v1"): VerificationResult {
  let report: RecordValue;
  try {
    report = record(decode(raw, 128 * 1024)); reportBounds(report);
  } catch { return rejectVerification("invalid_report"); }
  const contract = record(task.verification);
  try {
    require_(uint(report.schema_version) === 1n && report.profile === catalog.profile && report.kind === contract.kind, "report identity");
  } catch { return rejectVerification("report_identity_mismatch"); }
  if (executionReports === "v1") {
    const failed = executionFailure(report);
    if (failed) return failed;
  }
  try {
    require_(uint(report.effects) === BigInt(effects) && uint(report.fuel_remaining) <= uint(contract.max_fuel), "report budget");
  } catch { return rejectVerification("report_budget_mismatch"); }
  if (contract.require_verified && report.verified !== true) return rejectVerification("host_verification_missing");
  const expected = record(contract.result), resultType = verificationResultType(expected, catalog.semanticVersion);
  let actual: Json;
  try {
    const result = record(report.result);
    if (expected.kind === "observed") {
      require_(result.type === resultType && Object.hasOwn(result, "value"), "typed observed result");
      actual = scalar(result.value, resultType);
    } else {
      require_(result.type === resultType && Object.hasOwn(result, "value") && equal(result.value!, literalJson(expected)), "typed report result");
      actual = result.value!;
    }
  } catch { return rejectVerification("result_mismatch"); }
  if (expected.kind === "observed") {
    let evidence: Json;
    try { evidence = scalar(path(report, list(expected.path, 8, true)), resultType); }
    catch { return rejectVerification("result_evidence_invalid"); }
    if (!equal(actual, evidence)) return rejectVerification("result_evidence_mismatch");
  }
  for (const raw of list(contract.predicates, 64)) {
    const predicate = record(raw), relation = record(predicate.relation);
    let satisfied: boolean;
    try {
      const left = reportTerm(report, predicate.left, args), right = reportTerm(report, predicate.right, args);
      if (relation.kind === "equal") satisfied = equal(left, right);
      else if (relation.kind === "at_most") satisfied = uint(left) <= uint(right);
      else { const sum = uint(left) + uint(relation.amount); satisfied = sum <= U64_MAX && sum === uint(right); }
    } catch { return rejectVerification("predicate_evidence_invalid", predicate.id as string); }
    if (!satisfied) return rejectVerification("predicate_failed", predicate.id as string);
  }
  return VERIFICATION_SUCCESS;
}

