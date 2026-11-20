/** Compose a native catalog with owned, checked application task declarations. */
import { validate } from "./catalog.js";
import type { SemanticDescription } from "./declarations.js";
import { decode, fail, ownDeclaration, record, type Json } from "./json.js";

const CATALOG_LIMIT = 256 * 1024;

// Values are already owned primitives, arrays and null-prototype records. Never
// invoke caller toJSON hooks or stringify an integer through Number.
function encode(value: Json): string {
  if (typeof value === "bigint") return value.toString();
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(encode).join(",")}]`;
  return `{${Object.keys(value).map((key) => `${JSON.stringify(key)}:${encode(value[key]!)}`).join(",")}}`;
}

/**
 * Attach semantics v1/v2 to a base catalog without executing or granting Effects.
 * The base must not already contain semantics. Raw base JSON preserves duplicate
 * key and integer-token checks; object bases retain the existing safe-Number rule.
 * Author declarations additionally accept exact bigint u64 values. Return raw
 * JSON for providers/transports: ordinary JSON.stringify cannot encode bigint.
 */
export function defineSemanticCatalog(base: unknown, semantics: SemanticDescription): string {
  const catalog = record(decode(base, CATALOG_LIMIT));
  if (Object.hasOwn(catalog, "semantics")) fail("base catalog already has semantics");
  catalog.semantics = ownDeclaration(semantics, CATALOG_LIMIT);
  // Validate before encoding: e.g. a raw schema_version:1.0 must not become 1.
  validate(catalog);
  const raw = encode(catalog);
  // Enforce the complete catalog's byte, depth and node budgets after merging.
  decode(raw, CATALOG_LIMIT);
  return raw;
}
