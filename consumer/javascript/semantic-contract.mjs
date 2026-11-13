// Application-owned task metadata layered on the actual registered catalog.
// This application uses five controls and a publication task; it imports no
// reference frontend planner, generated source or private SDK implementation.
export const publicationTask = "publish_article";

const literal = (kind, value) => ({ kind: "literal", value: { kind, value } });
const text = (value) => literal("string", value);
const integer = (value) => literal("integer", value);
const boolean = (value) => literal("boolean", value);
const binding = (name) => ({ kind: "binding", name });
const field = (binding, name) => ({ kind: "field", binding, name });
const input = () => ({ kind: "input", name: "text" });
const equal = (left, right) => ({ kind: "equal", left, right });
const call = (binding, operation, arguments_ = {}) => ({
  kind: "call", binding, operation,
  arguments: Object.entries(arguments_).map(([name, value]) => ({ name, value })),
});
const guard = (...conditions) => ({ kind: "guard", conditions, failure: { kind: "boolean", value: false } });
const acknowledged = (name) => equal(binding(name), boolean(true));
const editable = (observed) => [equal(field(observed, "role"), text("text_field")),
  equal(field(observed, "enabled"), boolean(true)), equal(field(observed, "editable"), boolean(true))];
const path = (array, identifier, property) => [
  { kind: "field", name: array }, { kind: "match", field: "identifier", value: identifier },
  ...(property ? [{ kind: "field", name: property }] : []),
];
const scalar = (array, identifier, property, scalar_type) => ({ kind: "scalar", path: path(array, identifier, property), scalar_type });
const predicate = (id, left, right, relation = { kind: "equal" }) => ({ id, left, relation, right });

export function publicationCatalog(base) {
  // The caller supplies a freshly read native --describe record. JSON.clone
  // here is safe for the base catalog (only small version integers); it is not
  // a substitute for the SDK's exact semantic integer parser.
  const catalog = JSON.parse(JSON.stringify(base));
  if (catalog.profile !== "javascript.registered") throw new Error("unexpected registered host profile");
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
  catalog.semantics = {
    version: 1,
    operations: [
      ["gui.list", "List the application's registered control identifiers.", false],
      ["gui.find", "Resolve one exact registered control identifier in this workflow.", false],
      ["gui.observe", "Read fresh state from the registered application object.", false],
      ["gui.set_text", "Submit draft replacement; observe before trusting the value.", true],
      ["gui.click", "Submit a control action; publish updates its value and the status control.", true],
    ].map(([operation, summary, write]) => ({ operation, summary,
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
  return JSON.stringify(catalog);
}
