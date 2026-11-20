/** Bounded verification outcomes. Diagnostics carry no report values or exception text. */
export type VerificationFailureCode =
  | "invalid_report"
  | "report_identity_mismatch"
  | "execution_failed"
  | "execution_object_unavailable"
  | "execution_fuel_exhausted"
  | "execution_deadline_exceeded"
  | "execution_scope_closed"
  | "execution_cancelled"
  | "report_budget_mismatch"
  | "host_verification_missing"
  | "result_mismatch"
  | "result_evidence_invalid"
  | "result_evidence_mismatch"
  | "predicate_evidence_invalid"
  | "predicate_failed"
  | "completion_unavailable"
  | "plan_mismatch"
  | "report_mismatch"
  | "observation_invalid"
  | "before_observation_mismatch"
  | "after_observation_mismatch"
  | "application_bindings_changed"
  | "verification_interrupted"
  | "invalid_verification_result";

export interface VerificationDiagnostic {
  readonly code: VerificationFailureCode;
  /** An admitted predicate identifier, present only for predicate failures. */
  readonly predicate?: string;
}

export type VerificationResult =
  | { readonly verified: true }
  | { readonly verified: false; readonly diagnostic: VerificationDiagnostic };

const codes = new Set<VerificationFailureCode>([
  "invalid_report", "report_identity_mismatch", "report_budget_mismatch", "host_verification_missing",
  "execution_failed", "execution_object_unavailable", "execution_fuel_exhausted",
  "execution_deadline_exceeded", "execution_scope_closed", "execution_cancelled",
  "result_mismatch", "result_evidence_invalid", "result_evidence_mismatch", "predicate_evidence_invalid",
  "predicate_failed", "completion_unavailable", "plan_mismatch", "report_mismatch", "observation_invalid",
  "before_observation_mismatch", "after_observation_mismatch", "verification_interrupted", "invalid_verification_result",
  "application_bindings_changed",
]);
const invalid: VerificationResult = Object.freeze({ verified: false,
  diagnostic: Object.freeze({ code: "invalid_verification_result" }) });

export const VERIFICATION_SUCCESS: VerificationResult = Object.freeze({ verified: true });

function predicateCode(code: VerificationFailureCode): boolean {
  return code === "predicate_evidence_invalid" || code === "predicate_failed";
}

function predicateName(value: unknown): value is string {
  return typeof value === "string" && value.length <= 64 && /^[A-Za-z_][A-Za-z_0-9]*$/u.test(value)
    && !["fn", "true", "false", "none", "body"].includes(value) && !value.startsWith("_lese_");
}

/** Internal constructor; callers supply identifiers from their admitted contract. */
export function rejectVerification(code: VerificationFailureCode, predicate?: string): VerificationResult {
  if (!codes.has(code) || (predicateCode(code) ? !predicateName(predicate) : predicate !== undefined)) return invalid;
  return Object.freeze({ verified: false,
    diagnostic: Object.freeze({ code, ...(predicate === undefined ? {} : { predicate }) }) });
}

function fields(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)
    || ![Object.prototype, null].includes(Object.getPrototypeOf(value))) throw new Error();
  const keys = Reflect.ownKeys(value);
  if (keys.length > 2) throw new Error();
  const result: Record<string, unknown> = Object.create(null);
  for (const key of keys) {
    if (typeof key !== "string") throw new Error();
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (!descriptor?.enumerable || !("value" in descriptor)) throw new Error();
    result[key] = descriptor.value;
  }
  return result;
}

/** Own an exact plain result without invoking getters, coercion or serialization. */
export function admitVerificationResult(value: unknown): VerificationResult {
  try {
    const result = fields(value), keys = Object.keys(result);
    if (result.verified === true && keys.length === 1) return VERIFICATION_SUCCESS;
    if (result.verified !== false || keys.length !== 2 || !Object.hasOwn(result, "diagnostic")) return invalid;
    const diagnostic = fields(result.diagnostic), diagnosticKeys = Object.keys(diagnostic);
    if (typeof diagnostic.code !== "string" || !codes.has(diagnostic.code as VerificationFailureCode)) return invalid;
    const code = diagnostic.code as VerificationFailureCode;
    if (predicateCode(code)) {
      if (diagnosticKeys.length !== 2 || !Object.hasOwn(diagnostic, "predicate") || !predicateName(diagnostic.predicate)) return invalid;
      return rejectVerification(code, diagnostic.predicate);
    }
    return diagnosticKeys.length === 1 ? rejectVerification(code) : invalid;
  } catch { return invalid; }
}

/** Admit synchronous records before await can assimilate an arbitrary thenable. */
export function settleVerificationResult(value: unknown): VerificationResult | Promise<VerificationResult> {
  if (value instanceof Promise) {
    return Promise.prototype.then.call(value, admitVerificationResult) as Promise<VerificationResult>;
  }
  return admitVerificationResult(value);
}
