/** Host-authored semantic metadata. Runtime validation remains authoritative. */

/** Exact unsigned integer input: safe Number, or bigint through the authoring API. */
export type SemanticInteger = number | bigint;
export type SemanticScalarType = "integer" | "boolean" | "string" | "none" | "optional_string" | "string_list";

/** The literal vocabulary shared by semantic versions 1, 2 and 3. */
export type SemanticLiteral =
  | { readonly kind: "integer"; readonly value: SemanticInteger }
  | { readonly kind: "boolean"; readonly value: boolean }
  | { readonly kind: "string"; readonly value: string }
  | { readonly kind: "none" };

export interface SemanticOperationSemantics {
  readonly operation: string;
  readonly summary: string;
  readonly effect: "read" | "write";
  readonly completion: "value" | "submission";
}

export interface SemanticTaskInput {
  readonly name: string;
  readonly scalar_type: "string";
}

export type SemanticTerm =
  | { readonly kind: "literal"; readonly value: SemanticLiteral }
  | { readonly kind: "input"; readonly name: string }
  | { readonly kind: "binding"; readonly name: string }
  | { readonly kind: "field"; readonly binding: string; readonly name: string };

export interface SemanticTaskArgument {
  readonly name: string;
  readonly value: SemanticTerm;
}

export type SemanticCondition =
  | { readonly kind: "equal"; readonly left: SemanticTerm; readonly right: SemanticTerm }
  | { readonly kind: "contains"; readonly list: SemanticTerm; readonly value: SemanticTerm }
  | { readonly kind: "increased_by"; readonly before: SemanticTerm; readonly after: SemanticTerm; readonly amount: SemanticInteger }
  | { readonly kind: "optional_equals"; readonly optional: SemanticTerm; readonly value: SemanticTerm };

export type SemanticTaskStep =
  | { readonly kind: "call"; readonly binding: string; readonly operation: string; readonly arguments: readonly SemanticTaskArgument[] }
  | { readonly kind: "guard"; readonly conditions: readonly SemanticCondition[]; readonly failure: SemanticLiteral };

export type SemanticReportSegment =
  | { readonly kind: "field"; readonly name: string }
  | { readonly kind: "match"; readonly field: string; readonly value: string };

export type SemanticReportTerm =
  | { readonly kind: "literal"; readonly value: SemanticLiteral }
  | { readonly kind: "input"; readonly name: string }
  | { readonly kind: "path"; readonly path: readonly SemanticReportSegment[] }
  | { readonly kind: "length"; readonly path: readonly SemanticReportSegment[] }
  | { readonly kind: "scalar"; readonly path: readonly SemanticReportSegment[]; readonly scalar_type: SemanticScalarType };

export type SemanticReportRelation =
  | { readonly kind: "equal" }
  | { readonly kind: "at_most" }
  | { readonly kind: "increased_by"; readonly amount: SemanticInteger };

export interface SemanticReportPredicate {
  readonly id: string;
  readonly left: SemanticReportTerm;
  readonly relation: SemanticReportRelation;
  readonly right: SemanticReportTerm;
}

/** Version 2 ties the returned scalar to independently selected report evidence. */
export interface SemanticObservedResult {
  readonly kind: "observed";
  readonly scalar_type: "integer" | "boolean" | "string";
  readonly path: readonly SemanticReportSegment[];
}

export type SemanticResultContract = SemanticLiteral | SemanticObservedResult;

interface ReportContractBase {
  readonly kind: string;
  readonly max_fuel: SemanticInteger;
  readonly require_verified: boolean;
  readonly predicates: readonly SemanticReportPredicate[];
}

export interface SemanticReportContractV1 extends ReportContractBase {
  readonly result: SemanticLiteral;
}

export interface SemanticReportContractV2 extends ReportContractBase {
  readonly result: SemanticResultContract;
}

export type SemanticReportContract = SemanticReportContractV1 | SemanticReportContractV2;

interface TaskDescriptionBase {
  readonly id: string;
  readonly summary: string;
  readonly inputs: readonly SemanticTaskInput[];
  readonly steps: readonly SemanticTaskStep[];
  readonly result: SemanticTerm;
  readonly execution: "named_text" | "text" | "no_text";
}

export interface SemanticTaskDescriptionV1 extends TaskDescriptionBase {
  readonly verification: SemanticReportContractV1;
}

export interface SemanticTaskDescriptionV2 extends TaskDescriptionBase {
  readonly verification: SemanticReportContractV2;
}

/** Version 3 explicitly selects bounded, named string arguments. */
export interface SemanticTaskDescriptionV3 extends Omit<TaskDescriptionBase, "execution"> {
  readonly execution: "no_text" | "named_strings";
  readonly verification: SemanticReportContractV2;
}

export type SemanticTaskDescription = SemanticTaskDescriptionV1 | SemanticTaskDescriptionV2 | SemanticTaskDescriptionV3;

export interface SemanticDescriptionV1 {
  readonly version: 1;
  readonly operations: readonly SemanticOperationSemantics[];
  readonly tasks: readonly SemanticTaskDescriptionV1[];
}

export interface SemanticDescriptionV2 {
  readonly version: 2;
  readonly operations: readonly SemanticOperationSemantics[];
  readonly tasks: readonly SemanticTaskDescriptionV2[];
}

export interface SemanticDescriptionV3 {
  readonly version: 3;
  readonly operations: readonly SemanticOperationSemantics[];
  readonly tasks: readonly SemanticTaskDescriptionV3[];
}

/** Version 1 only admits fixed results; observed results require version 2 or 3. */
export type SemanticDescription = SemanticDescriptionV1 | SemanticDescriptionV2 | SemanticDescriptionV3;
