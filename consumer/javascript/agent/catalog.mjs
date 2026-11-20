// Application-authored v2 declarations use the public SDK writer and share the
// original publication task. No reference GUI or generated source is imported.
import { defineRegisteredTask, defineSemanticCatalog } from "@leselang/bridge";
import { publicationSemantics } from "../semantic-contract.mjs";

/** @typedef {import("@leselang/bridge").SemanticTerm} Term */
/** @typedef {import("@leselang/bridge").SemanticTaskStep} Step */
/** @typedef {import("@leselang/bridge").SemanticTaskDescriptionV2} TaskV2 */
/** @typedef {import("@leselang/bridge").SemanticReportPredicate} Predicate */
/** @typedef {import("@leselang/bridge").SemanticReportSegment} Segment */

const identifiers = ["draft", "publish", "status", "reset", "unrelated"];
const operations = ["gui.list", "gui.find", "gui.observe", "gui.set_text", "gui.click"];
/** @type {(value: string) => Term} */
const text = (value) => ({ kind: "literal", value: { kind: "string", value } });
/** @type {(name: string) => Term} */
const binding = (name) => ({ kind: "binding", name });
/** @type {(array: string, identifier: string, property?: string) => Segment[]} */
const path = (array, identifier, property) => {
  /** @type {Segment[]} */
  const segments = [{ kind: "field", name: array }, { kind: "match", field: "identifier", value: identifier }];
  if (property) segments.push({ kind: "field", name: property });
  return segments;
};
/** @type {(binding: string, operation: string, arguments_?: Readonly<Record<string, Term>>) => Step} */
const call = (binding, operation, arguments_ = {}) => ({
  kind: "call", binding, operation,
  arguments: Object.entries(arguments_).map(([name, value]) => ({ name, value })),
});

/** @type {(publication: TaskV2) => Predicate[]} */
function unchangedPredicates(publication) {
  // Keep the complete shape checks from the original contract, followed by
  // whole-record preservation for every object. There are 27 predicates.
  const shape = publication.verification.predicates.filter(({ id }) =>
    id.startsWith("objects_before_") || id.startsWith("objects_after_"));
  const predicates = structuredClone(shape);
  for (const identifier of identifiers) predicates.push({
    id: `${identifier}_unchanged`, relation: { kind: "equal" },
    left: { kind: "path", path: path("objects_before", identifier) },
    right: { kind: "path", path: path("objects_after", identifier) },
  });
  return predicates;
}

/** @type {(id: string, identifier: string, summary: string, publication: TaskV2) => TaskV2} */
function read(id, identifier, summary, publication) {
  return defineRegisteredTask({
    id, summary, inputs: [], execution: "no_text",
    failure: { kind: "string", value: "" },
    steps: [{ kind: "read_control", control: identifier, binding: "observed" }],
    result: { kind: "field", binding: "observed", name: "value" },
    verification: {
      kind: "javascript_task_run", max_fuel: 10_000, require_verified: false,
      result: { kind: "observed", scalar_type: "string", path: path("objects_after", identifier, "value") },
      predicates: unchangedPredicates(publication),
    },
  });
}

/** @param {string} baseRaw @returns {string} */
export function createPublicationAgentCatalog(baseRaw) {
  if (typeof baseRaw !== "string") throw new Error("registered catalog must be bounded JSON text");
  const original = publicationSemantics();
  const task = original.tasks[0];
  const publication = defineRegisteredTask({ ...task,
    failure: { kind: "boolean", value: false },
    steps: [
      call("ids", "gui.list"),
      { kind: "guard", failure: { kind: "boolean", value: false }, conditions: [
        { kind: "contains", list: binding("ids"), value: text("draft") },
        { kind: "contains", list: binding("ids"), value: text("publish") },
      ] },
      { kind: "set_control_text", control: "draft", binding: "draft_after", text: { kind: "input", name: "text" } },
      // Publication specifically requires the draft to remain editable. The
      // general text step also supports controls that disable themselves.
      { kind: "guard", failure: { kind: "boolean", value: false }, conditions: [
        { kind: "equal", left: { kind: "field", binding: "draft_after", name: "enabled" }, right: { kind: "literal", value: { kind: "boolean", value: true } } },
        { kind: "equal", left: { kind: "field", binding: "draft_after", name: "editable" }, right: { kind: "literal", value: { kind: "boolean", value: true } } },
      ] },
      { kind: "click_control", control: "publish", binding: "publish_after", before: "publish_before" },
      { kind: "guard", failure: { kind: "boolean", value: false }, conditions: [
        { kind: "equal", left: { kind: "field", binding: "publish_after", name: "value" }, right: { kind: "input", name: "text" } },
        { kind: "increased_by", before: { kind: "field", binding: "publish_before", name: "clicks" },
          after: { kind: "field", binding: "publish_after", name: "clicks" }, amount: 1 },
      ] },
    ],
    verification: { ...task.verification, kind: "javascript_task_run", require_verified: false },
  });
  // The version and all four tasks are declared directly; no published catalog
  // is parsed, modified or upgraded after admission.
  const raw = defineSemanticCatalog(baseRaw, {
    version: 2, operations: original.operations,
    tasks: [
      read("read_draft", "draft", "Read the application's current draft without changing any control.", publication),
      publication,
      read("read_publication", "publish", "Read the actual published text without changing any control.", publication),
      defineRegisteredTask({
        id: "check_publication", summary: "Discover the five publication controls and verify their state remains unchanged.",
        inputs: [], execution: "no_text", steps: [call("ids", "gui.list")], result: { kind: "literal", value: { kind: "none" } },
        failure: { kind: "none" },
        verification: { kind: "javascript_task_run", max_fuel: 10_000, require_verified: false,
          result: { kind: "none" }, predicates: unchangedPredicates(publication) },
      }),
    ],
  });
  // Only inspect names on already admitted output. Keep returning its original
  // exact integer bytes; never rewrite a catalog through JavaScript Number.
  /** @type {{profile: string, operations: {name: string}[]}} */
  const described = JSON.parse(raw);
  if (described.profile !== "javascript.registered" || described.operations.length !== operations.length
    || operations.some((name) => described.operations.filter((item) => item.name === name).length !== 1)) {
    throw new Error("unexpected registered worker catalog");
  }
  return raw;
}
