// Application-owned task metadata layered on the actual registered catalog.
// This application uses five controls and a publication task; it imports no
// reference frontend planner, generated source or private SDK implementation.
import { defineSemanticCatalog, prepareSemanticCatalog } from "@leselang/bridge";

/** @typedef {import("@leselang/bridge").SemanticDescriptionV1} DescriptionV1 */
/** @typedef {import("@leselang/bridge").SemanticTaskDescriptionV1} TaskV1 */
/** @typedef {import("@leselang/bridge").SemanticTerm} Term */
/** @typedef {import("@leselang/bridge").SemanticCondition} Condition */
/** @typedef {import("@leselang/bridge").SemanticTaskStep} Step */
/** @typedef {import("@leselang/bridge").SemanticReportSegment} Segment */
/** @typedef {import("@leselang/bridge").SemanticReportTerm} ReportTerm */
/** @typedef {import("@leselang/bridge").SemanticReportPredicate} Predicate */
/** @typedef {import("@leselang/bridge").SemanticReportRelation} Relation */
/** @typedef {Extract<Term, {kind: "literal"}>} LiteralTerm */

export const publicationTask = "publish_article";

/** @type {(value: string) => LiteralTerm} */
const text = (value) => ({ kind: "literal", value: { kind: "string", value } });
/** @type {(value: number | bigint) => LiteralTerm} */
const integer = (value) => ({ kind: "literal", value: { kind: "integer", value } });
/** @type {(value: boolean) => LiteralTerm} */
const boolean = (value) => ({ kind: "literal", value: { kind: "boolean", value } });
/** @type {(name: string) => Term} */
const binding = (name) => ({ kind: "binding", name });
/** @type {(binding: string, name: string) => Term} */
const field = (binding, name) => ({ kind: "field", binding, name });
/** @returns {Extract<Term, {kind: "input"}>} */
const input = () => ({ kind: "input", name: "text" });
/** @type {(left: Term, right: Term) => Condition} */
const equal = (left, right) => ({ kind: "equal", left, right });
/** @type {(binding: string, operation: string, arguments_?: Readonly<Record<string, Term>>) => Step} */
const call = (binding, operation, arguments_ = {}) => ({
  kind: "call", binding, operation,
  arguments: Object.entries(arguments_).map(([name, value]) => ({ name, value })),
});
/** @type {(...conditions: Condition[]) => Step} */
const guard = (...conditions) => ({ kind: "guard", conditions, failure: { kind: "boolean", value: false } });
/** @type {(name: string) => Condition} */
const acknowledged = (name) => equal(binding(name), boolean(true));
/** @type {(observed: string) => Condition[]} */
const editable = (observed) => [equal(field(observed, "role"), text("text_field")),
  equal(field(observed, "enabled"), boolean(true)), equal(field(observed, "editable"), boolean(true))];
/** @type {(array: string, identifier: string, property?: string) => Segment[]} */
const path = (array, identifier, property) => {
  /** @type {Segment[]} */
  const segments = [{ kind: "field", name: array }, { kind: "match", field: "identifier", value: identifier }];
  if (property) segments.push({ kind: "field", name: property });
  return segments;
};
/** @type {(array: string, identifier: string, property: string, scalar_type: import("@leselang/bridge").SemanticScalarType) => ReportTerm} */
const scalar = (array, identifier, property, scalar_type) => ({ kind: "scalar", path: path(array, identifier, property), scalar_type });
/** @type {(id: string, left: ReportTerm, right: ReportTerm, relation?: Relation) => Predicate} */
const predicate = (id, left, right, relation = { kind: "equal" }) => ({ id, left, relation, right });

/** Fresh application-owned declarations shared by the old and new agent entries.
 * @returns {DescriptionV1 & {readonly tasks: readonly [TaskV1]}}
 */
export function publicationSemantics() {
  /** @type {Predicate[]} */
  const predicates = [];
  for (const phase of ["objects_before", "objects_after"]) {
    predicates.push(predicate(`${phase}_count`, { kind: "length", path: [{ kind: "field", name: phase }] }, integer(5)));
    for (const identifier of ["draft", "publish", "status", "reset", "unrelated"]) {
      const role = ["draft", "status"].includes(identifier) ? "text_field" : "button";
      predicates.push(predicate(`${phase}_${identifier}_role`, scalar(phase, identifier, "role", "string"), text(role)));
      predicates.push(predicate(`${phase}_${identifier}_enabled`, scalar(phase, identifier, "enabled", "boolean"), boolean(true)));
    }
  }
  predicates.push(
    predicate("draft_value", scalar("objects_after", "draft", "value", "string"), input()),
    predicate("published_value", scalar("objects_after", "publish", "value", "string"), input()),
    predicate("publication_status", scalar("objects_after", "status", "value", "string"), text("published")),
    predicate("publication_count", scalar("objects_before", "publish", "clicks", "integer"),
      scalar("objects_after", "publish", "clicks", "integer"), { kind: "increased_by", amount: 1 }),
  );
  for (const identifier of ["reset", "unrelated"]) {
    predicates.push(predicate(`${identifier}_unchanged`,
      { kind: "path", path: path("objects_before", identifier) },
      { kind: "path", path: path("objects_after", identifier) }));
  }
  /** @type {readonly (readonly [string, string, boolean])[]} */
  const operationSummaries = [
    ["gui.list", "List the application's registered control identifiers.", false],
    ["gui.find", "Resolve one exact registered control identifier in this workflow.", false],
    ["gui.observe", "Read fresh state from the registered application object.", false],
    ["gui.set_text", "Submit draft replacement; observe before trusting the value.", true],
    ["gui.click", "Submit a control action; publish updates its value and the status control.", true],
  ];
  return {
    version: 1,
    operations: operationSummaries.map(([operation, summary, write]) => ({ operation, summary,
      effect: write ? "write" : "read", completion: write ? "submission" : "value" })),
    tasks: [{
      id: publicationTask,
      summary: "Publish the supplied article, verify its saved value and status, and preserve unrelated controls.",
      inputs: [{ name: "text", scalar_type: "string" }],
      execution: "text",
      steps: [
        call("ids", "gui.list"),
        guard({ kind: "contains", list: binding("ids"), value: text("draft") },
          { kind: "contains", list: binding("ids"), value: text("publish") }),
        call("draft_object", "gui.find", { name: text("draft") }),
        call("draft_before", "gui.observe", { target: binding("draft_object") }),
        guard(...editable("draft_before")),
        call("written", "gui.set_text", { target: binding("draft_object"),
          expected_revision: field("draft_before", "revision"), text: input() }),
        guard(acknowledged("written")),
        call("draft_after", "gui.observe", { target: binding("draft_object") }),
        guard(...editable("draft_after"), equal(field("draft_after", "value"), input())),
        call("publish_object", "gui.find", { name: text("publish") }),
        call("publish_before", "gui.observe", { target: binding("publish_object") }),
        guard(equal(field("publish_before", "role"), text("button")),
          equal(field("publish_before", "enabled"), boolean(true))),
        call("submitted", "gui.click", { target: binding("publish_object"), expected_revision: field("publish_before", "revision") }),
        guard(acknowledged("submitted")),
        call("publish_after", "gui.observe", { target: binding("publish_object") }),
        guard(equal(field("publish_after", "value"), input()),
          { kind: "increased_by", before: field("publish_before", "clicks"), after: field("publish_after", "clicks"), amount: 1 }),
      ],
      result: boolean(true),
      verification: { kind: "javascript_run", max_fuel: 10_000,
        result: { kind: "boolean", value: true }, require_verified: true, predicates },
    }],
  };
}

/** Retain the original Boolean publication/report contract through the public authoring API.
 * @param {unknown} base Fresh native base catalog, as raw JSON or plain data.
 * @returns {string}
 */
export function publicationCatalog(base) {
  const raw = defineSemanticCatalog(base, publicationSemantics());
  if (prepareSemanticCatalog(raw).profile !== "javascript.registered") throw new Error("unexpected registered host profile");
  return raw;
}
