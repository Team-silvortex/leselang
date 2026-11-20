import { defineRegisteredTaskV3, defineSemanticCatalog } from "@leselang/bridge";

/** @typedef {import("@leselang/bridge").SemanticReportTerm} Term */
/** @typedef {import("@leselang/bridge").SemanticReportSegment} Segment */
/** @typedef {import("@leselang/bridge").SemanticReportPredicate} Predicate */
/** @type {(phase: string, id?: string, field?: string) => Segment[]} */
const path = (phase, id, field) => {
  /** @type {Segment[]} */
  const result = [{ kind: "field", name: phase }];
  if (id !== undefined) result.push({ kind: "match", field: "identifier", value: id });
  if (field !== undefined) result.push({ kind: "field", name: field });
  return result;
};
/** @type {(id: string, left: Term, right: Term) => Predicate} */
const equal = (id, left, right) => ({ id, left, right, relation: { kind: "equal" } });
/** @type {(phase: string, id?: string, field?: string) => Term} */
const at = (phase, id, field) => ({ kind: "path", path: path(phase, id, field) });
const unchanged = equal("unchanged", at("objects_before"), at("objects_after"));

/** @param {unknown} base @returns {string} */
export function createArticleCatalog(base) {
  /** @param {string} id */
  const read = (id) => defineRegisteredTaskV3({
    id: `read_${id}`, summary: `Read the current article ${id} without changing state.`,
    inputs: [], execution: "no_text", failure: { kind: "string", value: "" },
    steps: [{ kind: "read_control", control: id, binding: "current" }],
    result: { kind: "field", binding: "current", name: "value" },
    verification: { kind: "javascript_task_run", max_fuel: 10_000, require_verified: false,
      result: { kind: "observed", scalar_type: "string", path: path("objects_after", id, "value") },
      predicates: [unchanged] },
  });
  return defineSemanticCatalog(base, {
    version: 3,
    operations: ["gui.list", "gui.find", "gui.observe", "gui.set_text", "gui.click"].map((operation) => ({
      operation, summary: `Use the application's registered ${operation} operation.`,
      effect: operation === "gui.click" || operation === "gui.set_text" ? "write" : "read",
      completion: operation === "gui.click" || operation === "gui.set_text" ? "submission" : "value",
    })),
    tasks: [read("title"), read("content"), defineRegisteredTaskV3({
      id: "publish_article", summary: "Publish the supplied title and content together, and preserve unrelated controls.",
      inputs: [{ name: "title", scalar_type: "string" }, { name: "content", scalar_type: "string" }],
      execution: "named_strings", failure: { kind: "boolean", value: false },
      steps: [
        { kind: "set_control_text", control: "title", binding: "title_after", text: { kind: "input", name: "title" } },
        { kind: "set_control_text", control: "content", binding: "content_after", text: { kind: "input", name: "content" } },
        { kind: "click_control", control: "publish", binding: "published" },
      ],
      result: { kind: "literal", value: { kind: "boolean", value: true } },
      verification: { kind: "javascript_task_run", max_fuel: 10_000, require_verified: false,
        result: { kind: "boolean", value: true }, predicates: [
          equal("title_input", at("objects_after", "title", "value"), { kind: "input", name: "title" }),
          equal("content_input", at("objects_after", "content", "value"), { kind: "input", name: "content" }),
          equal("saved_title", at("objects_after", "publish", "title"), { kind: "input", name: "title" }),
          equal("saved_content", at("objects_after", "publish", "value"), { kind: "input", name: "content" }),
          { id: "published_once", left: at("objects_before", "publish", "clicks"),
            relation: { kind: "increased_by", amount: 1 }, right: at("objects_after", "publish", "clicks") },
          equal("other_unchanged", at("objects_before", "other"), at("objects_after", "other")),
        ] },
    }), defineRegisteredTaskV3({
      id: "check_article", summary: "Check that the article's complete registered inventory is preserved.",
      inputs: [], execution: "no_text", failure: { kind: "none" },
      steps: [{ kind: "call", binding: "ids", operation: "gui.list", arguments: [] }],
      result: { kind: "literal", value: { kind: "none" } },
      verification: { kind: "javascript_task_run", max_fuel: 10_000, require_verified: false,
        result: { kind: "none" }, predicates: [unchanged] },
    })],
  });
}
