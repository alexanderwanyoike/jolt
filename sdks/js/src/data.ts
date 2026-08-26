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

/** One stored schema version and its opaque value. */
export type StoredSchemaValue = {
  readonly version: number;
  readonly value: unknown;
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

/** The immutable object supplied to a migration step. */
export type MigrationValue = Readonly<Record<string, unknown>>;

/** Source fields mapped to their new field names for a migration. */
export type MigrationRenames = Readonly<Record<string, string>>;

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
  return { ...(value as Record<string, unknown>) };
}

/**
 * Returns a migrated copy with every present source field moved to its new
 * name. Missing source fields are ignored; occupied destinations are rejected.
 */
function renameMigrationFields(
  value: MigrationValue,
  renames: MigrationRenames,
): Record<string, unknown> {
  const renamed = { ...value };
  const moves = Object.entries(renames).filter(([from, to]) => (
    Object.prototype.hasOwnProperty.call(value, from) && from !== to
  ));

  for (const [from] of moves) {
    delete renamed[from];
  }
  for (const [from, to] of moves) {
    if (Object.prototype.hasOwnProperty.call(renamed, to)) {
      throw new TypeError(`Cannot rename ${from} to existing field ${to}`);
    }
    // A data property keeps a destination named `__proto__` from changing
    // the result's prototype.
    Object.defineProperty(renamed, to, {
      configurable: true,
      enumerable: true,
      value: value[from],
      writable: true,
    });
  }
  return renamed;
}

class DefinedMigrationPlan implements MigrationPlan {
  constructor() {
    migrationSteps.set(this, new Map());
  }

  to(version: number, define: MigrationDefinition): this {
    requirePositiveVersion(version, "Migration target version");
    if (version < 2) {
      throw new RangeError("Migration target version must be at least 2");
    }
    const steps = migrationSteps.get(this)!;
    if (steps.has(version)) {
      throw new RangeError(`Migration to version ${version} is already defined`);
    }
    steps.set(version, value => define(migrationValue(value)));
    return this;
  }
}

/** Builds migrations and provides pure helpers for a current Schema Class. */
export const Migrations = {
  create: (): MigrationPlan => new DefinedMigrationPlan(),
  rename: renameMigrationFields,
} as const;

const resourceDefinition = Symbol("JoltDataResourceDefinition");
const policyKind = Symbol("JoltDataPolicyKind");

function policy<const TKind extends string>(kind: TKind) {
  return Object.freeze({ [policyKind]: kind });
}

/** Read scopes available to a Resource access declaration. */
export const Read = {
  OwnIdentity: policy("read:own-identity"),
  AnyIdentity: policy("read:any-identity"),
} as const;

/** Conflict policies for concurrent updates to the same field. */
export const UpdateConflict = {
  LastWriteWins: policy("update-conflict:last-write-wins"),
  Manual: policy("update-conflict:manual"),
} as const;

/** Conflict policies for a concurrent deletion and update. */
export const DeleteConflict = {
  DeleteWins: policy("delete-conflict:delete-wins"),
  UpdateWins: policy("delete-conflict:update-wins"),
  Manual: policy("delete-conflict:manual"),
} as const;

/**
 * Symbol-backed states for immutable Item snapshots. The class keeps every
 * static Symbol's unique-symbol type so direct equality checks narrow Items.
 */
export class State {
  static readonly Present = Symbol("JoltDataStatePresent");
  static readonly Deleted = Symbol("JoltDataStateDeleted");
  static readonly Missing = Symbol("JoltDataStateMissing");
  static readonly Unavailable = Symbol("JoltDataStateUnavailable");

  private constructor() {}
}

const referenceType = Symbol("JoltDataReferenceType");

/** A stable logical reference to one typed Item. */
export type Ref<T extends object> = {
  readonly identity: Identity;
  readonly path: string;
  readonly [referenceType]?: (value: T) => T;
};

/** Shared immutable state and narrowing behavior for an Item snapshot. */
export type ItemSnapshot<T extends object, TState extends symbol> = {
  readonly state: TState;
  readonly ref: Ref<T>;
  isPresent(): this is PresentItem<T>;
  isDeleted(): this is DeletedItem<T>;
};

/** A schema value whose nested object properties and arrays cannot be mutated. */
export type ImmutableValue<T> = T extends Date
  ? T
  : T extends readonly (infer TValue)[]
    ? readonly ImmutableValue<TValue>[]
    : T extends object
      ? { readonly [K in keyof T]: ImmutableValue<T[K]> }
      : T;

/** An immutable Item snapshot containing a current schema-valid value. */
export type PresentItem<T extends object> = ItemSnapshot<T, typeof State.Present> & {
  readonly value: ImmutableValue<T>;
};

/** An immutable Item snapshot whose current state is a Tombstone. */
export type DeletedItem<T extends object> = ItemSnapshot<T, typeof State.Deleted>;

/** An immutable Item snapshot for a logical reference with no observed record. */
export type MissingItem<T extends object> = ItemSnapshot<T, typeof State.Missing>;

/** An immutable Item snapshot whose current state cannot be determined. */
export type UnavailableItem<T extends object> = ItemSnapshot<T, typeof State.Unavailable>;

/** Any current immutable state of one logical Item. */
export type Item<T extends object> =
  | PresentItem<T>
  | DeletedItem<T>
  | MissingItem<T>
  | UnavailableItem<T>;

/** Operations an application requests for one Resource. */
export type ResourceAccess = {
  readonly read: typeof Read[keyof typeof Read];
  readonly create?: true;
  readonly update?: true;
  readonly delete?: true;
  readonly restore?: true;
};

/** Conflict behavior required for one Resource definition. */
export type ResourceConflicts = {
  readonly update: typeof UpdateConflict[keyof typeof UpdateConflict];
  readonly delete: typeof DeleteConflict[keyof typeof DeleteConflict];
};

/** Developer-facing access and conflict declarations for one Resource. */
export type ResourceOptions<TAccess extends ResourceAccess> = {
  readonly access: TAccess;
  readonly conflicts: ResourceConflicts;
};

/** Shared metadata and migration behavior for an unbound Resource. */
export type ResourceDefinition<
  T extends object,
  TAccess extends ResourceAccess,
  TKind extends "collection" | "document",
> = {
  readonly schema: SchemaClass<T>;
  readonly access: TAccess;
  readonly conflicts: ResourceConflicts;
  readonly migrate: (stored: StoredSchemaValue) => T;
  readonly [resourceDefinition]: TKind;
};

/** An unbound Collection definition created before it belongs to an App. */
export type CollectionDefinition<
  T extends object,
  TAccess extends ResourceAccess,
> = ResourceDefinition<T, TAccess, "collection">;

/** An unbound Document definition created before it belongs to an App. */
export type DocumentDefinition<
  T extends object,
  TAccess extends ResourceAccess,
> = ResourceDefinition<T, TAccess, "document">;

/** A Collection definition bound to its canonical App path prefix. */
export type BoundCollectionDefinition<
  T extends object,
  TAccess extends ResourceAccess,
> = CollectionDefinition<T, TAccess> & {
  readonly path: string;
};

/** A Document definition bound to its one canonical App path. */
export type BoundDocumentDefinition<
  T extends object,
  TAccess extends ResourceAccess,
> = DocumentDefinition<T, TAccess> & {
  readonly path: string;
};

function defineResource<
  T extends object,
  const TAccess extends ResourceAccess,
  const TKind extends "collection" | "document",
>(
  kind: TKind,
  schemaClass: SchemaClass<T>,
  options: ResourceOptions<TAccess>,
): ResourceDefinition<T, TAccess, TKind> {
  return {
    schema: schemaClass,
    access: options.access,
    conflicts: options.conflicts,
    migrate: stored => migrate(schemaClass, stored),
    [resourceDefinition]: kind,
  };
}

/** Defines an unbound typed Collection. App.create derives its path. */
export const Collection = {
  create: <T extends object, const TAccess extends ResourceAccess>(
    schemaClass: SchemaClass<T>,
    options: ResourceOptions<TAccess>,
  ): CollectionDefinition<T, TAccess> => defineResource("collection", schemaClass, options),
} as const;

/** Defines an unbound typed Document. App.create derives its path. */
export const Document = {
  create: <T extends object, const TAccess extends ResourceAccess>(
    schemaClass: SchemaClass<T>,
    options: ResourceOptions<TAccess>,
  ): DocumentDefinition<T, TAccess> => defineResource("document", schemaClass, options),
} as const;

/** Named unbound Resources accepted by App.create. */
export type AppDataDefinitions = Readonly<Record<
  string,
  | CollectionDefinition<object, ResourceAccess>
  | DocumentDefinition<object, ResourceAccess>
>>;

/** App data definitions after canonical paths have been derived. */
export type BoundAppData<TData extends AppDataDefinitions> = {
  readonly [K in keyof TData]: TData[K] extends CollectionDefinition<
    infer TValue,
    infer TAccess
  > ? BoundCollectionDefinition<TValue, TAccess>
    : TData[K] extends DocumentDefinition<infer TValue, infer TAccess>
      ? BoundDocumentDefinition<TValue, TAccess>
      : never;
};

/** Read operations shared by every connected Collection surface. */
export type CollectionReader<T extends object> = {
  get(ref: Ref<T>): Promise<Item<T>>;
};

/** Collection creation exposed only when declared in Resource access. */
export type CollectionCreator<T extends object> = {
  create(value: T): Promise<PresentItem<T>>;
};

/** A read-only Collection view bound to another identity. */
export type RemoteCollection<T extends object> = CollectionReader<T>;

/** Remote Collection reads exposed only for AnyIdentity access. */
export type CollectionRemoteReader<T extends object> = {
  for(identity: Identity): RemoteCollection<T>;
};

/** A connected Collection surface derived from its access declaration. */
export type CollectionResource<
  T extends object,
  TAccess extends ResourceAccess,
> = CollectionReader<T> & (
  TAccess extends { readonly create: true } ? CollectionCreator<T> : object
) & (
  TAccess["read"] extends typeof Read.AnyIdentity ? CollectionRemoteReader<T> : object
);

/** Read operations shared by every connected Document surface. */
export type DocumentReader<T extends object> = {
  get(): Promise<Item<T>>;
};

/** Document creation exposed only when declared in Resource access. */
export type DocumentCreator<T extends object> = {
  getOrCreate(value: T): Promise<PresentItem<T>>;
};

/** A read-only Document view bound to another identity. */
export type RemoteDocument<T extends object> = DocumentReader<T>;

/** Remote Document reads exposed only for AnyIdentity access. */
export type DocumentRemoteReader<T extends object> = {
  for(identity: Identity): RemoteDocument<T>;
};

/** A connected Document surface derived from its access declaration. */
export type DocumentResource<
  T extends object,
  TAccess extends ResourceAccess,
> = DocumentReader<T> & (
  TAccess extends { readonly create: true } ? DocumentCreator<T> : object
) & (
  TAccess["read"] extends typeof Read.AnyIdentity ? DocumentRemoteReader<T> : object
);

/** The connected Resource surface generated from one Resource definition. */
export type AppResource<TDefinition> = TDefinition extends CollectionDefinition<
  infer TValue,
  infer TAccess
> ? CollectionResource<TValue, TAccess>
  : TDefinition extends DocumentDefinition<infer TValue, infer TAccess>
    ? DocumentResource<TValue, TAccess>
    : object;

/** The direct named Resource surface returned by App.test or App.connect. */
export type AppInstance<TData extends AppDataDefinitions> = {
  readonly [K in keyof TData]: AppResource<TData[K]>;
};

/** Options for one fresh deterministic App test instance. */
export type AppTestOptions = {
  readonly identity?: Identity;
};

/** Shared deterministic state that can expose several identity-bound App views. */
export type AppTestWorld<TData extends AppDataDefinitions> = {
  as(identity: Identity): AppInstance<TData>;
};

/** A complete application definition with canonically bound Resources. */
export type AppDefinition<TData extends AppDataDefinitions> = {
  readonly id: string;
  readonly name: string;
  readonly namespace: string;
  readonly data: BoundAppData<TData>;
  test(options?: AppTestOptions): AppInstance<TData>;
  testWorld(): AppTestWorld<TData>;
};

function requirePathSegment(value: string, label: string): void {
  if (
    value.length === 0
    || value === "."
    || value === ".."
    || /[/\s?#]/u.test(value)
  ) {
    throw new TypeError(`${label} must be one valid path segment: ${value}`);
  }
}

type TestWorldState = {
  readonly store: Map<string, object>;
  nextId: number;
};

function createTestWorldState(): TestWorldState {
  return { store: new Map(), nextId: 0 };
}

function testStoreKey(ref: Pick<Ref<object>, "identity" | "path">): string {
  return `${ref.identity}\u0000${ref.path}`;
}

function createRef<T extends object>(identity: Identity, path: string): Ref<T> {
  return Object.freeze({ identity, path }) as Ref<T>;
}

function trueIsPresent<T extends object>(this: Item<T>): this is PresentItem<T> {
  return true;
}

function falseIsPresent<T extends object>(this: Item<T>): this is PresentItem<T> {
  return false;
}

function falseIsDeleted<T extends object>(this: Item<T>): this is DeletedItem<T> {
  return false;
}

function freezeValue<T>(value: T, seen = new WeakSet<object>()): ImmutableValue<T> {
  if (value === null || typeof value !== "object" || seen.has(value)) {
    return value as ImmutableValue<T>;
  }
  seen.add(value);
  for (const nested of Object.values(value)) {
    freezeValue(nested, seen);
  }
  return Object.freeze(value) as ImmutableValue<T>;
}

function presentItem<T extends object>(ref: Ref<T>, value: T): PresentItem<T> {
  return Object.freeze({
    state: State.Present,
    ref,
    value: freezeValue(value),
    isPresent: trueIsPresent,
    isDeleted: falseIsDeleted,
  });
}

function missingItem<T extends object>(ref: Ref<T>): MissingItem<T> {
  return Object.freeze({
    state: State.Missing,
    ref,
    isPresent: falseIsPresent,
    isDeleted: falseIsDeleted,
  });
}

function createTestCollection<T extends object, TAccess extends ResourceAccess>(
  resource: BoundCollectionDefinition<T, TAccess>,
  identity: Identity,
  state: TestWorldState,
  nextId: () => string,
  remote = false,
): CollectionResource<T, TAccess> {
  const collection: CollectionReader<T>
    & Partial<CollectionCreator<T>>
    & Partial<CollectionRemoteReader<T>> = {
    async get(ref) {
      if (ref.identity !== identity || !ref.path.startsWith(`${resource.path}/`)) {
        throw new TypeError("Collection reference does not belong to this Resource view");
      }
      const value = state.store.get(testStoreKey(ref)) as T | undefined;
      return value === undefined
        ? missingItem(ref)
        : presentItem(ref, parse(resource.schema, value));
    },
  };
  if (!remote && resource.access.create === true) {
    collection.create = async (input) => {
      const value = parse(resource.schema, input);
      const ref = createRef<T>(identity, `${resource.path}/${nextId()}`);
      state.store.set(testStoreKey(ref), value);
      return presentItem(ref, parse(resource.schema, value));
    };
  }
  if (!remote && resource.access.read === Read.AnyIdentity) {
    collection.for = remoteIdentity => (
      createTestCollection(resource, remoteIdentity, state, nextId, true)
    );
  }
  return Object.freeze(collection) as CollectionResource<T, TAccess>;
}

function createTestDocument<T extends object, TAccess extends ResourceAccess>(
  resource: BoundDocumentDefinition<T, TAccess>,
  identity: Identity,
  state: TestWorldState,
  remote = false,
): DocumentResource<T, TAccess> {
  const ref = createRef<T>(identity, resource.path);
  const document: DocumentReader<T>
    & Partial<DocumentCreator<T>>
    & Partial<DocumentRemoteReader<T>> = {
    async get() {
      const value = state.store.get(testStoreKey(ref)) as T | undefined;
      return value === undefined
        ? missingItem(ref)
        : presentItem(ref, parse(resource.schema, value));
    },
  };
  if (!remote && resource.access.create === true) {
    document.getOrCreate = async (input) => {
      const existing = state.store.get(testStoreKey(ref)) as T | undefined;
      if (existing !== undefined) {
        return presentItem(ref, parse(resource.schema, existing));
      }
      const value = parse(resource.schema, input);
      state.store.set(testStoreKey(ref), value);
      return presentItem(ref, parse(resource.schema, value));
    };
  }
  if (!remote && resource.access.read === Read.AnyIdentity) {
    document.for = remoteIdentity => createTestDocument(resource, remoteIdentity, state, true);
  }
  return Object.freeze(document) as DocumentResource<T, TAccess>;
}

function createTestApp<TData extends AppDataDefinitions>(
  data: BoundAppData<TData>,
  options: AppTestOptions = {},
  state: TestWorldState = createTestWorldState(),
): AppInstance<TData> {
  const identity = options.identity ?? "test.jolt";
  const opaqueId = () => `jlt_${(++state.nextId).toString(36).padStart(12, "0")}`;

  return Object.fromEntries(Object.entries(data).map(([name, resource]) => [
    name,
    resource[resourceDefinition] === "collection"
      ? createTestCollection(resource, identity, state, opaqueId)
      : createTestDocument(resource, identity, state),
  ])) as AppInstance<TData>;
}

/** Composes Resource definitions into one application definition. */
export const App = {
  create: <const TData extends AppDataDefinitions>(options: {
    readonly id: string;
    readonly name: string;
    readonly namespace: string;
    readonly data: TData;
  }): AppDefinition<TData> => {
    requirePathSegment(options.namespace, "App namespace");
    const data = Object.fromEntries(Object.entries(options.data).map(([name, resource]) => {
      requirePathSegment(name, "Resource name");
      return [
        name,
        {
          ...resource,
          path: `/${options.namespace}/${name}`,
        },
      ];
    })) as BoundAppData<TData>;
    return {
      id: options.id,
      name: options.name,
      namespace: options.namespace,
      data,
      test: testOptions => createTestApp(data, testOptions),
      testWorld: () => {
        const state = createTestWorldState();
        return Object.freeze({
          as: (identity: Identity) => createTestApp(data, { identity }, state),
        });
      },
    };
  },
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
  stored: StoredSchemaValue,
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
