import { boundedText, fault, fields } from "../agent/data.js";

/** Application-owned scalar operations shared by GUI and CLI hosts. */
export type OperationScalarType = "string" | "integer" | "boolean" | "none" | "optional_string" | "string_list";
type ScalarValues = {
  string: string;
  integer: bigint;
  boolean: boolean;
  none: null;
  optional_string: string | null;
  string_list: readonly string[];
};
export type OperationValue = ScalarValues[OperationScalarType];
export type OperationWireValue = Exclude<OperationValue, bigint>;
export type OperationParameters = Readonly<Record<string, OperationScalarType>>;
type Arguments<P extends OperationParameters> = { readonly [K in keyof P]: ScalarValues[P[K]] };

export interface OperationSchema<P extends OperationParameters = OperationParameters, R extends OperationScalarType = OperationScalarType> {
  readonly name: string;
  readonly parameters: P;
  readonly result: R;
  readonly capability: string;
}

export interface OperationContext {
  readonly signal: AbortSignal;
  /** Recheck the live capability immediately before a synchronous application mutation. */
  commit<T>(mutation: () => T): T;
}

export interface OperationDefinition<P extends OperationParameters = OperationParameters, R extends OperationScalarType = OperationScalarType>
  extends OperationSchema<P, R> {
  handle(args: Arguments<P>, context: OperationContext): ScalarValues[R] | Promise<ScalarValues[R]>;
}

/** Infer a handler's named parameter and result types from its declaration. */
export function defineOperation<const P extends OperationParameters, const R extends OperationScalarType>(
  declaration: OperationDefinition<P, R>,
): OperationDefinition<P, R> {
  return declaration;
}

export interface OperationAuthority {
  readonly signal: AbortSignal;
  allows(capability: string): boolean;
}

export interface OperationRegistry {
  describe(): readonly OperationSchema[];
  dispatch(name: string, arguments_: unknown, authority: OperationAuthority): Promise<OperationWireValue>;
}

const encoder = new TextEncoder();
const types = new Set<unknown>(["string", "integer", "boolean", "none", "optional_string", "string_list"]);
const maxInteger = (1n << 64n) - 1n;

function scalarType(value: unknown): OperationScalarType {
  if (!types.has(value)) fault("invalid_operation_type");
  return value as OperationScalarType;
}

function identifier(name: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]{0,63}$/u.test(name)
    && !["fn", "true", "false", "none", "body"].includes(name) && !name.startsWith("_lese_");
}

function path(value: unknown): string {
  if (typeof value !== "string" || value.length > 128 || !value.includes(".")
    || !value.split(".").every(identifier)) fault("invalid_operation_name");
  return value;
}

/** Refuse holes, accessors, extra keys and custom prototypes before reading elements. */
function arrayData(value: unknown, limit: number): unknown[] {
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype
    || value.length > limit || Reflect.ownKeys(value).length !== value.length + 1) fault("invalid_operation_array");
  const owned: unknown[] = [];
  for (let index = 0; index < value.length; index++) {
    const descriptor = Object.getOwnPropertyDescriptor(value, index);
    if (!descriptor?.enumerable || !("value" in descriptor)) fault("invalid_operation_array");
    owned.push(descriptor.value);
  }
  return owned;
}

function simpleValue(type: Exclude<OperationScalarType, "integer">, value: unknown): OperationWireValue {
  switch (type) {
    case "optional_string": if (value === null) return null;
      return boundedText(value, 4096, "operation_string");
    case "string": return boundedText(value, 4096, "operation_string");
    case "boolean": if (typeof value === "boolean") return value; break;
    case "none": if (value === null) return null; break;
    case "string_list": {
      const items = arrayData(value, 64);
      let bytes = 0;
      return Object.freeze(items.map((item) => {
        const text = boundedText(item, 4096, "operation_string");
        bytes += encoder.encode(text).length;
        if (bytes > 4096) fault("invalid_operation_string_list");
        return text;
      }));
    }
  }
  return fault("invalid_operation_value");
}

/** Decode a worker scalar using its declared type. Integers retain all 64 bits. */
export function readOperationValue<T extends OperationScalarType>(type: T, wire: unknown): ScalarValues[T];
export function readOperationValue(type: OperationScalarType, wire: unknown): OperationValue {
  if (type !== "integer") return simpleValue(type, wire);
  if (typeof wire !== "string" || !/^(?:0|[1-9][0-9]{0,19})$/u.test(wire)) fault("invalid_operation_integer");
  const integer = BigInt(wire);
  if (integer > maxInteger) fault("invalid_operation_integer");
  return integer;
}

function writeValue(type: OperationScalarType, value: unknown): OperationWireValue {
  if (type !== "integer") return simpleValue(type, value);
  if (typeof value !== "bigint" || value < 0n || value > maxInteger) fault("invalid_operation_integer");
  return value.toString();
}

interface Entry {
  readonly schema: OperationSchema;
  readonly handle: OperationDefinition["handle"];
}

function entry(value: unknown): Entry {
  const def = fields(value, ["name", "parameters", "result", "capability", "handle"]);
  const params = Object.entries(fields(def.parameters));
  if (params.length > 8 || params.some(([name]) => !identifier(name))) fault("invalid_operation_parameters");
  const parameters: Record<string, OperationScalarType> = Object.create(null);
  for (const [name, type] of params) parameters[name] = scalarType(type);
  const schema = Object.freeze({ name: path(def.name), parameters: Object.freeze(parameters),
    result: scalarType(def.result), capability: path(def.capability) });
  if (typeof def.handle !== "function") fault("invalid_operation_handler");
  return { schema, handle: def.handle.bind(value) };
}

async function dispatch(entry: Entry, raw: unknown, authority: OperationAuthority): Promise<OperationWireValue> {
  const { parameters, capability, result } = entry.schema;
  const supplied = fields(raw, Object.keys(parameters));
  const args: Record<string, OperationValue> = Object.create(null);
  for (const [name, type] of Object.entries(parameters)) args[name] = readOperationValue(type, supplied[name]);

  const signal = authority?.signal;
  const policy = authority?.allows;
  if (!signal || typeof signal.aborted !== "boolean" || typeof signal.addEventListener !== "function"
    || typeof signal.removeEventListener !== "function" || typeof policy !== "function") fault("invalid_operation_authority");
  const allows = policy.bind(authority);
  let live = true, checking = false, committing = false, handlerActive = true;
  function check(): void {
    if (!live || signal.aborted) fault("operation_scope_closed");
    if (checking) fault("operation_reentrant_authorization");
    checking = true;
    try {
      if (allows(capability) !== true) fault("operation_permission_denied");
      if (signal.aborted) fault("operation_scope_closed");
    } catch (error) {
      live = false;
      throw error;
    } finally { checking = false; }
  }
  const context: OperationContext = Object.freeze({
    signal,
    commit<T>(mutation: () => T): T {
      if (!handlerActive) fault("operation_scope_closed");
      if (committing) fault("operation_reentrant_commit");
      check();
      committing = true;
      try {
        const value = mutation();
        if (value instanceof Promise) {
          void Promise.prototype.then.call(value, undefined, () => {});
          fault("operation_async_commit");
        }
        if (value !== null && (typeof value === "object" || typeof value === "function") && "then" in value) {
          fault("operation_async_commit");
        }
        return value;
      } finally { committing = false; }
    },
  });
  try {
    check();
    const invocation = entry.handle(Object.freeze(args), context);
    // Validate synchronous values directly so a malformed thenable cannot run application code.
    const value = invocation instanceof Promise ? await invocation : invocation;
    handlerActive = false;
    const wire = writeValue(result, value);
    check();
    return wire;
  } finally { handlerActive = false; live = false; }
}

export function createOperationRegistry(operations: readonly OperationDefinition[]): OperationRegistry {
  const declarations = arrayData(operations, 32);
  if (!declarations.length) fault("empty_operation_registry");
  const entries = new Map<string, Entry>();
  for (const declaration of declarations) {
    const item = entry(declaration);
    if (entries.has(item.schema.name)) fault("duplicate_operation");
    entries.set(item.schema.name, item);
  }
  const schemas = Object.freeze(Array.from(entries.values(), (item) => item.schema));
  return Object.freeze({
    describe: () => schemas,
    async dispatch(name: string, raw: unknown, authority: OperationAuthority) {
      const item = entries.get(name);
      if (!item) fault("unknown_operation");
      return dispatch(item, raw, authority);
    },
  });
}
