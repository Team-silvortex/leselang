/** Exact, bounded JSON ownership and primitive semantic value checks. */
export type Json = null | boolean | string | number | bigint | Json[] | RecordValue;
export interface RecordValue { [name: string]: Json }
export type ScalarType = "integer" | "boolean" | "string" | "none" | "optional_string" | "string_list";

export const encoder = new TextEncoder();
export const U64_MAX = 18_446_744_073_709_551_615n;
export function fail(reason: string): never { throw new Error(`invalid_semantics: ${reason}`); }
export function require_(valid: boolean, reason: string): void { if (!valid) fail(reason); }

export function text(value: unknown, limit: number, nonempty = false): string {
  if (typeof value !== "string" || value.length > limit || (nonempty && value.length === 0)) fail("text bounds or type");
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(++index);
      if (!(next >= 0xdc00 && next <= 0xdfff)) fail("invalid UTF-16");
    } else if (unit >= 0xdc00 && unit <= 0xdfff) fail("invalid UTF-16");
  }
  if (encoder.encode(value).length > limit) fail("UTF-8 text bound");
  return value;
}

/** Preserve integer tokens exactly; never recover a u64 from a rounded Number. */
export function decode(value: unknown, limit: number): Json {
  return decodeValue(value, limit, false);
}

/** Authoring alone admits exact u64 bigint fields; ordinary JSON ingress does not. */
export function ownDeclaration(value: unknown, limit: number): Json {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail("semantic declaration record expected");
  return decodeValue(value, limit, true);
}

function decodeValue(value: unknown, limit: number, allowBigInt: boolean): Json {
  let nodes = 0;
  const node = (depth: number) => {
    if (++nodes > 65_536 || depth > 32) fail("JSON structure bound");
  };
  if (typeof value === "string") {
    const source = text(value, limit, true);
    let index = 0;
    const whitespace = () => { while (/[ \t\r\n]/u.test(source[index] ?? "!")) index += 1; };
    const string = (): string => {
      const start = index++;
      while (index < source.length) {
        const char = source[index++]!;
        if (char === "\\") { index += 1; continue; }
        if (char === '"') {
          try { return text(JSON.parse(source.slice(start, index)), limit); }
          catch { return fail("JSON string"); }
        }
      }
      return fail("unterminated JSON string");
    };
    const parse = (depth: number): Json => {
      node(depth); whitespace();
      const char = source[index];
      if (char === '"') return string();
      if (char === "[") {
        index += 1; whitespace();
        const values: Json[] = [];
        if (source[index] === "]") { index += 1; return values; }
        while (true) {
          values.push(parse(depth + 1)); whitespace();
          if (source[index] === "]") { index += 1; return values; }
          if (source[index++] !== ",") fail("JSON array separator");
        }
      }
      if (char === "{") {
        index += 1; whitespace();
        const fields: RecordValue = Object.create(null);
        if (source[index] === "}") { index += 1; return fields; }
        while (true) {
          whitespace();
          if (source[index] !== '"') fail("JSON object key");
          const key = string();
          if (Object.hasOwn(fields, key)) fail("duplicate JSON key");
          whitespace();
          if (source[index++] !== ":") fail("JSON object separator");
          fields[key] = parse(depth + 1); whitespace();
          if (source[index] === "}") { index += 1; return fields; }
          if (source[index++] !== ",") fail("JSON object separator");
        }
      }
      for (const [token, result] of [["true", true], ["false", false], ["null", null]] as const) {
        if (source.startsWith(token, index)) { index += token.length; return result; }
      }
      const match = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/u.exec(source.slice(index));
      if (!match) fail("JSON value");
      index += match[0].length;
      // Float/exponent lexemes remain distinct from integer tokens, as in serde.
      if (/[.eE]/u.test(match[0]) || match[0] === "-0") {
        const number = Number(match[0]);
        if (!Number.isFinite(number)) fail("JSON number range");
        return number;
      }
      return BigInt(match[0]);
    };
    const result = parse(0); whitespace();
    if (index !== source.length) fail("trailing JSON data");
    return result;
  }

  const seen = new Set<object>();
  let bytes = 0;
  const spend = (count: number) => { if ((bytes += count) > limit) fail("JSON byte bound"); };
  const stringBytes = (value: string) => encoder.encode(JSON.stringify(value)).length;
  const copy = (value: unknown, depth: number): Json => {
    node(depth);
    if (value === null || typeof value === "boolean") { spend(value === null || value === true ? 4 : 5); return value; }
    if (typeof value === "string") { const checked = text(value, limit); spend(stringBytes(checked)); return checked; }
    if (typeof value === "number") {
      if (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value))) fail("unsafe JSON number");
      spend(String(value).length);
      return Number.isInteger(value) ? BigInt(value) : value;
    }
    if (allowBigInt && typeof value === "bigint") {
      uint(value); spend(value.toString().length); return value;
    }
    if (typeof value !== "object" || seen.has(value)) fail("JSON value or cycle");
    seen.add(value);
    try {
      const keys = Reflect.ownKeys(value);
      if (Array.isArray(value)) {
        if (value.length > 65_536 || keys.length !== value.length + 1) fail("JSON array properties");
        spend(2 + Math.max(0, value.length - 1));
        return Array.from({ length: value.length }, (_, index) => {
          const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
          if (!descriptor?.enumerable || !("value" in descriptor)) fail("JSON array accessor or hole");
          return copy(descriptor.value, depth + 1);
        });
      }
      if (![Object.prototype, null].includes(Object.getPrototypeOf(value))) fail("JSON record prototype");
      spend(2 + Math.max(0, keys.length - 1) + keys.length);
      const fields: RecordValue = Object.create(null);
      for (const key of keys) {
        if (typeof key !== "string") fail("JSON symbol key");
        const descriptor = Object.getOwnPropertyDescriptor(value, key);
        if (!descriptor?.enumerable || !("value" in descriptor)) fail("JSON record accessor");
        const checked = text(key, limit); spend(stringBytes(checked));
        fields[checked] = copy(descriptor.value, depth + 1);
      }
      return fields;
    } finally { seen.delete(value); }
  };
  return copy(value, 0);
}

export function record(value: Json | undefined, exact?: readonly string[]): RecordValue {
  if (value === null || typeof value !== "object" || Array.isArray(value)) fail("record expected");
  if (exact) {
    const keys = Object.keys(value);
    if (keys.length !== exact.length || keys.some((key) => !exact.includes(key))) fail("unknown or missing semantic field");
  }
  return value;
}
export function list(value: Json | undefined, max: number, nonempty = false): Json[] {
  if (!Array.isArray(value) || value.length > max || (nonempty && value.length === 0)) fail("collection bound or type");
  return value;
}
export function uint(value: Json | undefined, max = U64_MAX): bigint {
  if (typeof value !== "bigint" || value < 0n || value > max) fail("unsigned integer type or bound");
  return value;
}
export function bool(value: Json | undefined): boolean {
  if (typeof value !== "boolean") fail("Boolean expected");
  return value;
}
