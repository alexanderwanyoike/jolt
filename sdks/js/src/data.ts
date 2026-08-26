/** A decorated application value class used as both runtime schema and TypeScript type. */
export type SchemaClass<T extends object> = new () => T;

/** A Jolt identity address such as `alice.jolt`. */
export type Identity = string;

type ScalarFieldKind = "string" | "number" | "boolean" | "identity" | "dateTime";

const ISO_DATE_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;

type ValueDefinition =
  | {
      readonly kind: ScalarFieldKind;
    }
  | {
      readonly kind: "schema";
      readonly schemaClass: SchemaClass<object>;
    }
  | {
      readonly kind: "array";
      readonly item: ValueDefinition;
    };

type FieldDefinition = ValueDefinition & {
  readonly optional: boolean;
};

/** Options shared by Schema Class field decorators. */
export type FieldOptions = {
  readonly optional?: boolean;
};

/** Options for a Schema Class. Versions are positive and start at one. */
export type SchemaOptions = {
  readonly version: number;
  readonly migrations?: MigrationPlan;
};

/** A value failed validation against a Schema Class. */
export class SchemaValidationError extends Error {
  constructor(
    readonly field: string,
    message: string,
  ) {
    super(`${field} ${message}`);
    this.name = "SchemaValidationError";
  }
}

/** An older stored value could not be migrated into the current Schema Class. */
export class SchemaMigrationError extends Error {
  constructor(
    readonly fromVersion: number,
    readonly toVersion: number,
    message: string,
    options?: ErrorOptions,
  ) {
    super(message, options);
    this.name = "SchemaMigrationError";
  }
}

type MigrationTransform = (value: unknown) => unknown;

/**
 * The immutable object supplied to a migration step.
 *
 * Spread it for a pure transform, or use {@link MigrationValue.rename} for a
 * simple field rename.
 */
export type MigrationValue = Readonly<Record<string, unknown>> & {
  rename(from: string, to: string): Record<string, unknown>;
};

/** One deterministic, side-effect-free migration into its declared version. */
export type MigrationDefinition = (value: MigrationValue) => unknown;

/** A chain of migrations, keyed by the version each step produces. */
export interface MigrationPlan {
  to(version: number, define: MigrationDefinition): this;
}

const migrationSteps = new WeakMap<MigrationPlan, Map<number, MigrationTransform>>();

function requirePositiveVersion(version: number, label: string): void {
  if (!Number.isSafeInteger(version) || version < 1) {
    throw new RangeError(`${label} must be a positive integer`);
  }
}

function migrationValue(value: unknown): MigrationValue {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("Migration value must be an object");
  }
  const source = { ...(value as Record<string, unknown>) };
  Object.defineProperty(source, "rename", {
    enumerable: false,
    value: (from: string, to: string) => {
      const renamed = { ...source };
      renamed[to] = renamed[from];
      delete renamed[from];
      return renamed;
    },
  });
  return source as MigrationValue;
}

class DefinedMigrationPlan implements MigrationPlan {
  constructor() {
    migrationSteps.set(this, new Map());
  }

  to(version: number, define: MigrationDefinition): this {
    requirePositiveVersion(version, "Migration target version");
    const steps = migrationSteps.get(this)!;
    if (steps.has(version)) {
      throw new RangeError(`Migration to version ${version} is already defined`);
    }
    steps.set(version, value => define(migrationValue(value)));
    return this;
  }
}

/** Builds a separate migration history for a current Schema Class. */
export const Migrations = {
  create: (): MigrationPlan => new DefinedMigrationPlan(),
} as const;

const fieldsBySchema = new WeakMap<Function, Map<string | symbol, FieldDefinition>>();
const optionsBySchema = new WeakMap<Function, SchemaOptions>();
const valueDefinition = Symbol("JoltDataFieldDefinition");
const optionalField = Symbol("JoltDataOptionalField");

/** A property decorator produced by one of the typed {@link Field} helpers. */
export type SchemaFieldDecorator = PropertyDecorator;

/** A primitive field helper that can also describe an Array's item type. */
export type SchemaFieldFactory = (options?: FieldOptions) => SchemaFieldDecorator;

/** A primitive or nested Schema Class descriptor accepted by {@link Field.array}. */
export type ArrayFieldItem = SchemaFieldFactory | SchemaFieldDecorator;

type DefinedFieldDecorator = SchemaFieldDecorator & {
  readonly [valueDefinition]: ValueDefinition;
  readonly [optionalField]: boolean;
};

function field(definition: ValueDefinition, options: FieldOptions = {}): DefinedFieldDecorator {
  const decorator: PropertyDecorator = (target, propertyKey) => {
    if (target === undefined) {
      throw new TypeError(
        "Jolt Schema Classes require TypeScript experimentalDecorators; enable it in tsconfig.json",
      );
    }
    const schema = target.constructor;
    let fields = fieldsBySchema.get(schema);
    if (fields === undefined) {
      fields = new Map();
      fieldsBySchema.set(schema, fields);
    }
    fields.set(propertyKey, {
      ...definition,
      optional: options.optional ?? false,
    });
  };
  return Object.assign(decorator, {
    [valueDefinition]: definition,
    [optionalField]: options.optional ?? false,
  });
}

function schema(options: SchemaOptions): ClassDecorator {
  requirePositiveVersion(options.version, "Schema version");
  return (target) => {
    optionsBySchema.set(target, options);
  };
}

function parse<T extends object>(schemaClass: SchemaClass<T>, input: unknown): T {
  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw new SchemaValidationError("$", "must be an object");
  }
  if (!optionsBySchema.has(schemaClass)) {
    throw new TypeError("Class is not decorated with @Schema");
  }

  const source = input as Record<PropertyKey, unknown>;
  const value = Object.create(schemaClass.prototype) as T;
  const output = value as Record<PropertyKey, unknown>;

  for (const [propertyKey, definition] of fieldsBySchema.get(schemaClass) ?? []) {
    const fieldValue = source[propertyKey];
    if (definition.optional && fieldValue === undefined) {
      continue;
    }
    output[propertyKey] = parseValue(definition, fieldValue, String(propertyKey));
  }

  return value;
}

function migrate<T extends object>(
  schemaClass: SchemaClass<T>,
  stored: { readonly version: number; readonly value: unknown },
): T {
  const options = optionsBySchema.get(schemaClass);
  if (options === undefined) {
    throw new TypeError("Class is not decorated with @Schema");
  }
  requirePositiveVersion(stored.version, "Stored schema version");
  if (stored.version > options.version) {
    throw new SchemaMigrationError(
      stored.version,
      options.version,
      `Cannot migrate schema version ${stored.version} to older version ${options.version}`,
    );
  }
  if (stored.version === options.version) {
    return parse(schemaClass, stored.value);
  }

  let value = stored.value;
  for (let version = stored.version + 1; version <= options.version; version += 1) {
    const step = options.migrations === undefined
      ? undefined
      : migrationSteps.get(options.migrations)?.get(version);
    if (step === undefined) {
      throw new SchemaMigrationError(
        version - 1,
        version,
        `Missing migration to schema version ${version}`,
      );
    }
    try {
      value = step(value);
    } catch (cause) {
      throw new SchemaMigrationError(
        version - 1,
        version,
        `Migration to schema version ${version} failed`,
        { cause },
      );
    }
  }

  try {
    return parse(schemaClass, value);
  } catch (cause) {
    throw new SchemaMigrationError(
      stored.version,
      options.version,
      `Migrated value does not match schema version ${options.version}`,
      { cause },
    );
  }
}

function parseValue(definition: ValueDefinition, input: unknown, path: string): unknown {
  if (definition.kind === "schema") {
    if (input === null || typeof input !== "object" || Array.isArray(input)) {
      throw new SchemaValidationError(path, "must be an object");
    }
    return parse(definition.schemaClass, input);
  }
  if (definition.kind === "array") {
    if (!Array.isArray(input)) {
      throw new SchemaValidationError(path, "must be an array");
    }
    return input.map((item, index) => parseValue(definition.item, item, `${path}[${index}]`));
  }
  if (definition.kind === "string") {
    if (typeof input !== "string") {
      throw new SchemaValidationError(path, "must be a string");
    }
    return input;
  }
  if (definition.kind === "number") {
    if (typeof input !== "number" || !Number.isFinite(input)) {
      throw new SchemaValidationError(path, "must be a finite number");
    }
    return input;
  }
  if (definition.kind === "boolean") {
    if (typeof input !== "boolean") {
      throw new SchemaValidationError(path, "must be a boolean");
    }
    return input;
  }
  if (definition.kind === "identity") {
    if (typeof input !== "string" || input.length === 0) {
      throw new SchemaValidationError(path, "must be an identity");
    }
    return input;
  }

  const parsed = input instanceof Date
    ? new Date(input.getTime())
    : typeof input === "string" && ISO_DATE_TIME.test(input)
      ? new Date(input)
      : null;
  if (parsed === null || Number.isNaN(parsed.getTime())) {
    throw new SchemaValidationError(path, "must be a date-time");
  }
  return parsed;
}

function arrayItemDefinition(item: ArrayFieldItem): ValueDefinition {
  const candidate = item as DefinedFieldDecorator;
  const decorator = valueDefinition in candidate
    ? candidate
    : (item as SchemaFieldFactory)() as DefinedFieldDecorator;
  if (decorator[optionalField]) {
    throw new TypeError("Set optional on the array field instead of its items");
  }
  return decorator[valueDefinition];
}

/**
 * Declares a Schema Class and exposes direct validation and migration helpers
 * for advanced use and focused schema tests.
 */
export const Schema = Object.assign(schema, { parse, migrate });

/** Typed field decorators for Schema Classes. */
export const Field: {
  readonly string: SchemaFieldFactory;
  readonly number: SchemaFieldFactory;
  readonly boolean: SchemaFieldFactory;
  readonly identity: SchemaFieldFactory;
  readonly dateTime: SchemaFieldFactory;
  readonly schema: <T extends object>(
    schemaClass: SchemaClass<T>,
    options?: FieldOptions,
  ) => SchemaFieldDecorator;
  readonly array: (
    item: ArrayFieldItem,
    options?: FieldOptions,
  ) => SchemaFieldDecorator;
} = {
  string: (options: FieldOptions = {}) => field({ kind: "string" }, options),
  number: (options: FieldOptions = {}) => field({ kind: "number" }, options),
  boolean: (options: FieldOptions = {}) => field({ kind: "boolean" }, options),
  identity: (options: FieldOptions = {}) => field({ kind: "identity" }, options),
  dateTime: (options: FieldOptions = {}) => field({ kind: "dateTime" }, options),
  schema: <T extends object>(schemaClass: SchemaClass<T>, options: FieldOptions = {}) => field({
    kind: "schema",
    schemaClass: schemaClass as SchemaClass<object>,
  }, options),
  array: (item: ArrayFieldItem, options: FieldOptions = {}) => field({
    kind: "array",
    item: arrayItemDefinition(item),
  }, options),
} as const;
