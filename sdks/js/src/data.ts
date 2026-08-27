import { makeId } from "./client.js";
import type { JoltSdk } from "./client.js";
import {
  isContentUnavailableError,
  isJoltUnavailableError,
  JoltApiError,
} from "./errors.js";

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
 * Symbol-backed kinds used when inspecting a derived App access plan. The
 * class preserves unique-symbol types for direct equality narrowing.
 */
export class ResourceKind {
  static readonly Collection = Symbol("JoltDataResourceCollection");
  static readonly Document = Symbol("JoltDataResourceDocument");

  private constructor() {}
}

/** One Collection or Document discriminant used throughout Resource definitions. */
export type ResourceKindValue =
  | typeof ResourceKind.Collection
  | typeof ResourceKind.Document;

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

/** A mutation could not safely proceed because the Item's state is unknown. */
export class ItemUnavailableError extends Error {
  readonly ref: Pick<Ref<object>, "identity" | "path">;

  constructor(ref: Pick<Ref<object>, "identity" | "path">) {
    super(`Item state is unavailable: ${ref.identity}${ref.path}`);
    this.name = "ItemUnavailableError";
    this.ref = ref;
  }
}

/** A mutation observed an older Item revision than the record currently has. */
export class ConflictError extends Error {
  readonly ref: Pick<Ref<object>, "identity" | "path">;

  constructor(ref: Pick<Ref<object>, "identity" | "path">) {
    super(`Item changed since it was read: ${ref.identity}${ref.path}`);
    this.name = "ConflictError";
    this.ref = ref;
  }
}

/** A shallow update: omitted fields remain and supplied fields replace whole values. */
export type ShallowPatch<T extends object> = {
  readonly [K in keyof T]?: T[K];
};

/** Shared immutable state and narrowing behavior for an Item snapshot. */
export type ItemSnapshot<
  T extends object,
  TState extends symbol,
  TAccess extends ResourceAccess = ResourceAccess,
> = {
  readonly state: TState;
  readonly ref: Ref<T>;
  isPresent(): this is PresentItem<T, TAccess>;
  isDeleted(): this is DeletedItem<T, TAccess>;
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
export type PresentItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
> = ItemSnapshot<T, typeof State.Present, TAccess> & {
  readonly value: ImmutableValue<T>;
} & (TAccess extends { readonly update: true } ? {
  update(patch: ShallowPatch<T>): Promise<PresentItem<T, TAccess>>;
  replace(value: T): Promise<PresentItem<T, TAccess>>;
} : object) & (TAccess extends { readonly delete: true } ? {
  delete(): Promise<DeletedItem<T, TAccess>>;
} : object);

/** An immutable Item snapshot whose current state is a Tombstone. */
export type DeletedItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
> = ItemSnapshot<T, typeof State.Deleted, TAccess>;

/** An immutable Item snapshot for a logical reference with no observed record. */
export type MissingItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
> = ItemSnapshot<T, typeof State.Missing, TAccess>;

/** An immutable Item snapshot whose current state cannot be determined. */
export type UnavailableItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
> = ItemSnapshot<T, typeof State.Unavailable, TAccess>;

/** Any current immutable state of one logical Item. */
export type Item<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
> =
  | PresentItem<T, TAccess>
  | DeletedItem<T, TAccess>
  | MissingItem<T, TAccess>
  | UnavailableItem<T, TAccess>;

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
  TKind extends ResourceKindValue,
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
> = ResourceDefinition<T, TAccess, typeof ResourceKind.Collection>;

/** An unbound Document definition created before it belongs to an App. */
export type DocumentDefinition<
  T extends object,
  TAccess extends ResourceAccess,
> = ResourceDefinition<T, TAccess, typeof ResourceKind.Document>;

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

const accessOperations = new Set(["read", "create", "update", "delete", "restore"]);
const mutationAccessOperations = ["create", "update", "delete", "restore"] as const;

function validatedResourceAccess<TAccess extends ResourceAccess>(access: TAccess): TAccess {
  if (access === null || typeof access !== "object" || Array.isArray(access)) {
    throw new TypeError("Resource access must be an object");
  }
  for (const operation of Object.keys(access)) {
    if (!accessOperations.has(operation)) {
      throw new TypeError(`Unknown Resource access operation: ${operation}`);
    }
  }
  if (access.read !== Read.OwnIdentity && access.read !== Read.AnyIdentity) {
    throw new TypeError("Resource access read must be Read.OwnIdentity or Read.AnyIdentity");
  }
  for (const operation of mutationAccessOperations) {
    if (access[operation] !== undefined && access[operation] !== true) {
      throw new TypeError(`Resource access ${operation} must be true when declared`);
    }
  }
  return Object.freeze({ ...access });
}

function validatedResourceConflicts(conflicts: ResourceConflicts): ResourceConflicts {
  if (conflicts === null || typeof conflicts !== "object" || Array.isArray(conflicts)) {
    throw new TypeError("Resource conflicts must be an object");
  }
  if (!(Object.values(UpdateConflict) as readonly unknown[]).includes(conflicts.update)) {
    throw new TypeError("Resource update conflict must be a value from UpdateConflict");
  }
  if (!(Object.values(DeleteConflict) as readonly unknown[]).includes(conflicts.delete)) {
    throw new TypeError("Resource delete conflict must be a value from DeleteConflict");
  }
  return Object.freeze({ ...conflicts });
}

function defineResource<
  T extends object,
  const TAccess extends ResourceAccess,
  const TKind extends ResourceKindValue,
>(
  kind: TKind,
  schemaClass: SchemaClass<T>,
  options: ResourceOptions<TAccess>,
): ResourceDefinition<T, TAccess, TKind> {
  const access = validatedResourceAccess(options.access);
  const conflicts = validatedResourceConflicts(options.conflicts);
  return Object.freeze({
    schema: schemaClass,
    access,
    conflicts,
    migrate: stored => migrate(schemaClass, stored),
    [resourceDefinition]: kind,
  });
}

/** Defines an unbound typed Collection. App.create derives its path. */
export const Collection = {
  create: <T extends object, const TAccess extends ResourceAccess>(
    schemaClass: SchemaClass<T>,
    options: ResourceOptions<TAccess>,
  ): CollectionDefinition<T, TAccess> => (
    defineResource(ResourceKind.Collection, schemaClass, options)
  ),
} as const;

/** Creation was refused because the logical Item is explicitly deleted. */
export class DeletedError extends Error {
  readonly ref: Pick<Ref<object>, "identity" | "path">;

  constructor(ref: Pick<Ref<object>, "identity" | "path">) {
    super(`Item is deleted: ${ref.identity}${ref.path}`);
    this.name = "DeletedError";
    this.ref = ref;
  }
}

/** Defines an unbound typed Document. App.create derives its path. */
export const Document = {
  create: <T extends object, const TAccess extends ResourceAccess>(
    schemaClass: SchemaClass<T>,
    options: ResourceOptions<TAccess>,
  ): DocumentDefinition<T, TAccess> => (
    defineResource(ResourceKind.Document, schemaClass, options)
  ),
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
export type CollectionReader<T extends object, TAccess extends ResourceAccess> = {
  get(ref: Ref<T>): Promise<Item<T, TAccess>>;
};

/** Collection creation exposed only when declared in Resource access. */
export type CollectionCreator<T extends object, TAccess extends ResourceAccess> = {
  create(value: T): Promise<PresentItem<T, TAccess>>;
};

/** A read-only Collection view bound to another identity. */
export type RemoteCollection<T extends object> = CollectionReader<T, {
  readonly read: typeof Read.AnyIdentity;
}>;

/** Remote Collection reads exposed only for AnyIdentity access. */
export type CollectionRemoteReader<T extends object> = {
  for(identity: Identity): RemoteCollection<T>;
};

/** A connected Collection surface derived from its access declaration. */
export type CollectionResource<
  T extends object,
  TAccess extends ResourceAccess,
> = CollectionReader<T, TAccess> & (
  TAccess extends { readonly create: true } ? CollectionCreator<T, TAccess> : object
) & (
  TAccess["read"] extends typeof Read.AnyIdentity ? CollectionRemoteReader<T> : object
);

/** Read operations shared by every connected Document surface. */
export type DocumentReader<T extends object, TAccess extends ResourceAccess> = {
  get(): Promise<Item<T, TAccess>>;
};

/** Document creation exposed only when declared in Resource access. */
export type DocumentCreator<T extends object, TAccess extends ResourceAccess> = {
  getOrCreate(value: T): Promise<PresentItem<T, TAccess>>;
};

/** A read-only Document view bound to another identity. */
export type RemoteDocument<T extends object> = DocumentReader<T, {
  readonly read: typeof Read.AnyIdentity;
}>;

/** Remote Document reads exposed only for AnyIdentity access. */
export type DocumentRemoteReader<T extends object> = {
  for(identity: Identity): RemoteDocument<T>;
};

/** A connected Document surface derived from its access declaration. */
export type DocumentResource<
  T extends object,
  TAccess extends ResourceAccess,
> = DocumentReader<T, TAccess> & (
  TAccess extends { readonly create: true } ? DocumentCreator<T, TAccess> : object
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

/** Low-level authorized client operations used by the Data SDK connection seam. */
export type DataSdkClient = Pick<
  JoltSdk,
  | "publishJson"
  | "read"
  | "readContent"
  | "readRecord"
  | "resolve"
  | "updateRecord"
  | "deleteRecord"
>;

/**
 * Advanced connection seam for an already-authorized Jolt client. It keeps
 * host bootstrap separate from typed Resource behavior.
 */
export type AppConnectOptions = {
  readonly identity: Identity;
  readonly client: DataSdkClient;
};

/** Shared deterministic state that can expose several identity-bound App views. */
export type AppTestWorld<TData extends AppDataDefinitions> = {
  as(identity: Identity): AppInstance<TData>;
};

/** High-level node behavior required by one declared Resource. */
export type ResourceRequirement = {
  readonly resource: string;
  readonly kind: ResourceKindValue;
  readonly access: Readonly<ResourceAccess>;
};

/** High-level authority requested for one canonically scoped Resource. */
export type ResourceGrantPlan = {
  readonly resource: string;
  readonly path: string;
  readonly access: Readonly<ResourceAccess>;
};

/**
 * Inspectable connection input derived from an App's Resource declarations.
 * Requirements and Grants are index-aligned in declared Resource order.
 */
export type AppAccessPlan = {
  readonly requirements: readonly ResourceRequirement[];
  readonly grants: readonly ResourceGrantPlan[];
};

/** A complete application definition with canonically bound Resources. */
export type AppDefinition<TData extends AppDataDefinitions> = {
  readonly id: string;
  readonly name: string;
  readonly namespace: string;
  readonly data: BoundAppData<TData>;
  readonly accessPlan: AppAccessPlan;
  connect(options: AppConnectOptions): Promise<AppInstance<TData>>;
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
  readonly store: Map<string, BackendPresentRecord | BackendDeletedRecord>;
  nextId: number;
  nextMutationId: number;
  nextRevision: number;
};

function createTestWorldState(): TestWorldState {
  return {
    store: new Map(),
    nextId: 0,
    nextMutationId: 0,
    nextRevision: 0,
  };
}

function testStoreKey(ref: Pick<Ref<object>, "identity" | "path">): string {
  return `${ref.identity}\u0000${ref.path}`;
}

function createRef<T extends object>(identity: Identity, path: string): Ref<T> {
  return Object.freeze({ identity, path }) as Ref<T>;
}

function trueIsPresent<T extends object, TAccess extends ResourceAccess>(
  this: Item<T, TAccess>,
): this is PresentItem<T, TAccess> {
  return true;
}

function falseIsPresent<T extends object, TAccess extends ResourceAccess>(
  this: Item<T, TAccess>,
): this is PresentItem<T, TAccess> {
  return false;
}

function falseIsDeleted<T extends object, TAccess extends ResourceAccess>(
  this: Item<T, TAccess>,
): this is DeletedItem<T, TAccess> {
  return false;
}

function trueIsDeleted<T extends object, TAccess extends ResourceAccess>(
  this: Item<T, TAccess>,
): this is DeletedItem<T, TAccess> {
  return true;
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

function presentItem<T extends object, TAccess extends ResourceAccess>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue>,
  backend: DataBackend,
  ref: Ref<T>,
  record: BackendPresentRecord,
  mutable: boolean,
): PresentItem<T, TAccess> {
  const migrated = migratedStoredValue(resource.schema, record.stored);
  const value = migrated.value;
  const item: Record<string, unknown> = {
    state: State.Present,
    ref,
    value: freezeValue(value),
    isPresent: trueIsPresent,
    isDeleted: falseIsDeleted,
  };
  const revision = mutable ? record.revision : null;
  if (resource.access.update === true && revision !== null) {
    const commit = async (stored: StoredSchemaValue) => {
      const next = await backend.update(
        ref,
        stored,
        revision,
        backend.nextMutationId(),
      );
      return presentItem(resource, backend, ref, next, mutable);
    };
    item.update = async (patch: ShallowPatch<T>) => {
      const stored = patchedStoredValue(
        resource.schema,
        migrated.stored,
        value,
        patch,
      );
      return commit(stored);
    };
    item.replace = async (input: T) => {
      const current = currentStoredValue(resource.schema, input);
      return commit(current.stored);
    };
  }
  if (resource.access.delete === true && revision !== null) {
    item.delete = async () => {
      const deleted = await backend.delete(ref, revision, backend.nextMutationId());
      return deletedItem(ref, { backend, revision: deleted.revision });
    };
  }
  return Object.freeze(item) as PresentItem<T, TAccess>;
}

function missingItem<T extends object, TAccess extends ResourceAccess>(
  ref: Ref<T>,
): MissingItem<T, TAccess> {
  return Object.freeze({
    state: State.Missing,
    ref,
    isPresent: falseIsPresent,
    isDeleted: falseIsDeleted,
  });
}

const deletedItemMutationContexts = new WeakMap<object, {
  readonly backend: DataBackend;
  readonly revision: string;
}>();

function deletedItem<T extends object, TAccess extends ResourceAccess>(
  ref: Ref<T>,
  mutationContext?: {
    readonly backend: DataBackend;
    readonly revision: string;
  },
): DeletedItem<T, TAccess> {
  const item = Object.freeze({
    state: State.Deleted,
    ref,
    isPresent: falseIsPresent,
    isDeleted: trueIsDeleted,
  });
  if (mutationContext !== undefined) {
    deletedItemMutationContexts.set(item, mutationContext);
  }
  return item;
}

function unavailableItem<T extends object, TAccess extends ResourceAccess>(
  ref: Ref<T>,
): UnavailableItem<T, TAccess> {
  return Object.freeze({
    state: State.Unavailable,
    ref,
    isPresent: falseIsPresent,
    isDeleted: falseIsDeleted,
  });
}

function decodeStoredSchemaValue(input: unknown): StoredSchemaValue | null {
  if (input === null || typeof input !== "object" || Array.isArray(input)) return null;
  const candidate = input as Record<string, unknown>;
  if (
    !Number.isSafeInteger(candidate.version)
    || (candidate.version as number) < 1
    || !Object.prototype.hasOwnProperty.call(candidate, "value")
  ) {
    return null;
  }
  return { version: candidate.version as number, value: candidate.value };
}

function requireStoredSchemaValue(input: unknown): StoredSchemaValue {
  const stored = decodeStoredSchemaValue(input);
  if (stored === null) {
    throw new SchemaValidationError("$", "must be a versioned schema value");
  }
  return stored;
}

function currentStoredValue<T extends object>(
  schemaClass: SchemaClass<T>,
  input: T,
): { readonly stored: StoredSchemaValue; readonly value: T } {
  const options = optionsBySchema.get(schemaClass);
  if (options === undefined) {
    throw new TypeError("Class is not decorated with @Schema");
  }
  const value = parse(schemaClass, input);
  return {
    stored: { version: options.version, value },
    value,
  };
}

function patchedStoredValue<T extends object>(
  schemaClass: SchemaClass<T>,
  previous: StoredSchemaValue,
  currentValue: T,
  patch: ShallowPatch<T>,
): StoredSchemaValue {
  if (patch === null || typeof patch !== "object" || Array.isArray(patch)) {
    throw new SchemaValidationError("$", "patch must be an object");
  }
  const raw = previous.value as Record<PropertyKey, unknown>;
  const candidate = { ...currentValue, ...patch } as T;
  const current = currentStoredValue(schemaClass, candidate);
  const parsed = current.stored.value as Record<PropertyKey, unknown>;
  const next = { ...raw };
  const currentVersion = current.stored.version;

  for (const propertyKey of fieldsBySchema.get(schemaClass)?.keys() ?? []) {
    const explicitlyPatched = Object.prototype.hasOwnProperty.call(patch, propertyKey);
    const canKeepRaw = previous.version === currentVersion
      && Object.prototype.hasOwnProperty.call(raw, propertyKey)
      && !explicitlyPatched;
    if (canKeepRaw) continue;
    if (Object.prototype.hasOwnProperty.call(parsed, propertyKey)) {
      next[propertyKey] = parsed[propertyKey];
    } else {
      delete next[propertyKey];
    }
  }

  return { version: currentVersion, value: next };
}

type BackendPresentRecord = {
  readonly stored: StoredSchemaValue;
  readonly revision: string | null;
};

type BackendDeletedRecord = {
  readonly state: typeof State.Deleted;
  readonly revision: string | null;
};

type BackendReadResult =
  | BackendPresentRecord
  | BackendDeletedRecord
  | typeof State.Missing
  | typeof State.Unavailable;

function isBackendDeletedRecord(
  record: BackendPresentRecord | BackendDeletedRecord,
): record is BackendDeletedRecord {
  return "state" in record && record.state === State.Deleted;
}

type DataBackend = {
  readonly identity: Identity;
  nextId(): string;
  nextMutationId(): string;
  read<T extends object>(ref: Ref<T>): Promise<BackendReadResult>;
  write<T extends object>(ref: Ref<T>, stored: StoredSchemaValue): Promise<BackendPresentRecord>;
  update<T extends object>(
    ref: Ref<T>,
    stored: StoredSchemaValue,
    revision: string,
    mutationId: string,
  ): Promise<BackendPresentRecord>;
  delete<T extends object>(
    ref: Ref<T>,
    revision: string,
    mutationId: string,
  ): Promise<BackendDeletedRecord & { readonly revision: string }>;
  for(identity: Identity): DataBackend;
};

function createTestBackend(state: TestWorldState, identity: Identity): DataBackend {
  return {
    identity,
    nextId: () => `jlt_${(++state.nextId).toString(36).padStart(12, "0")}`,
    nextMutationId: () => `mut_${(++state.nextMutationId).toString(36).padStart(12, "0")}`,
    async read(ref) {
      return state.store.get(testStoreKey(ref)) ?? State.Missing;
    },
    async write(ref, stored) {
      const record = { stored, revision: `revision_${++state.nextRevision}` };
      state.store.set(testStoreKey(ref), record);
      return record;
    },
    async update(ref, stored, revision) {
      const key = testStoreKey(ref);
      const current = state.store.get(key);
      if (
        current === undefined
        || isBackendDeletedRecord(current)
        || current.revision !== revision
      ) {
        throw new ConflictError(ref);
      }
      const record = { stored, revision: `revision_${++state.nextRevision}` };
      state.store.set(key, record);
      return record;
    },
    async delete(ref, revision) {
      const key = testStoreKey(ref);
      const current = state.store.get(key);
      if (
        current === undefined
        || isBackendDeletedRecord(current)
        || current.revision !== revision
      ) {
        throw new ConflictError(ref);
      }
      const record = {
        state: State.Deleted,
        revision: `revision_${++state.nextRevision}`,
      } as const;
      state.store.set(key, record);
      return record;
    },
    for: remoteIdentity => createTestBackend(state, remoteIdentity),
  };
}

function createConnectedBackend(
  options: AppConnectOptions,
  localIdentity: Identity = options.identity,
): DataBackend {
  return {
    identity: options.identity,
    nextId: () => makeId("jlt"),
    nextMutationId: () => makeId("mut"),
    async read(ref) {
      if (ref.identity !== localIdentity) {
        let resolved;
        try {
          resolved = await options.client.resolve(ref);
        } catch (error) {
          if (error instanceof JoltApiError && error.code === "path_tombstoned") {
            return { state: State.Deleted, revision: null };
          }
          return State.Unavailable;
        }
        const versioned = await options.client.readContent(
          resolved.contentId,
          ref,
          resolved.latestSequence,
          value => ({ value }),
        );
        if (versioned === null) return State.Unavailable;
        return {
          stored: requireStoredSchemaValue(versioned.value.value),
          revision: null,
        };
      }
      let record;
      try {
        record = await options.client.readRecord(ref);
      } catch (error) {
        if (
          isJoltUnavailableError(error) ||
          isContentUnavailableError(error)
        ) {
          return State.Unavailable;
        }
        throw error;
      }
      if (record.state === "missing") return State.Missing;
      if (record.state === "deleted") {
        return { state: State.Deleted, revision: record.revision };
      }
      return backendRecord(record.bytes, record.revision);
    },
    async write(ref, stored) {
      const published = await options.client.publishJson(ref.path, stored);
      if (published.revision !== undefined) {
        return { stored, revision: published.revision };
      }
      const record = await options.client.readRecord(ref);
      if (record.state !== "present") {
        throw new ItemUnavailableError(ref);
      }
      return backendRecord(record.bytes, record.revision);
    },
    async update(ref, stored, revision, mutationId) {
      try {
        const record = await options.client.updateRecord(
          ref,
          stored,
          { revision, mutationId },
        );
        return backendRecord(record.bytes, record.revision);
      } catch (error) {
        if (error instanceof JoltApiError && error.code === "record_conflict") {
          throw new ConflictError(ref);
        }
        throw error;
      }
    },
    async delete(ref, revision, mutationId) {
      try {
        const record = await options.client.deleteRecord(
          ref,
          { revision, mutationId },
        );
        return { state: State.Deleted, revision: record.revision };
      } catch (error) {
        if (error instanceof JoltApiError && error.code === "record_conflict") {
          throw new ConflictError(ref);
        }
        throw error;
      }
    },
    for: identity => createConnectedBackend({ ...options, identity }, localIdentity),
  };
}

function backendRecord(bytes: readonly number[], revision: string): BackendPresentRecord {
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes)));
  } catch {
    throw new SchemaValidationError("$", "must be valid JSON");
  }
  return { stored: requireStoredSchemaValue(parsed), revision };
}

async function readItem<T extends object, TAccess extends ResourceAccess>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue>,
  backend: DataBackend,
  ref: Ref<T>,
  mutable: boolean,
): Promise<Item<T, TAccess>> {
  const stored = await backend.read(ref);
  if (stored === State.Missing) return missingItem(ref);
  if (stored === State.Unavailable) return unavailableItem(ref);
  if (isBackendDeletedRecord(stored)) {
    return deletedItem(
      ref,
      mutable && stored.revision !== null
        ? { backend, revision: stored.revision }
        : undefined,
    );
  }
  return presentItem(resource, backend, ref, stored, mutable);
}

type ResourceViewOptions = {
  readonly remote?: boolean;
};

function createCollection<T extends object, TAccess extends ResourceAccess>(
  resource: BoundCollectionDefinition<T, TAccess>,
  backend: DataBackend,
  options: ResourceViewOptions = {},
): CollectionResource<T, TAccess> {
  const remote = options.remote ?? false;
  const collection: CollectionReader<T, TAccess>
    & Partial<CollectionCreator<T, TAccess>>
    & Partial<CollectionRemoteReader<T>> = {
    async get(ref) {
      if (
        ref.identity !== backend.identity
        || !ref.path.startsWith(`${resource.path}/`)
      ) {
        throw new TypeError("Collection reference does not belong to this Resource view");
      }
      return readItem(resource, backend, ref, !remote);
    },
  };
  if (!remote && resource.access.create === true) {
    collection.create = async (input) => {
      const { stored } = currentStoredValue(resource.schema, input);
      const ref = createRef<T>(backend.identity, `${resource.path}/${backend.nextId()}`);
      const record = await backend.write(ref, stored);
      return presentItem(resource, backend, ref, record, true);
    };
  }
  if (!remote && resource.access.read === Read.AnyIdentity) {
    collection.for = identity => createCollection(
      resource,
      backend.for(identity),
      { remote: true },
    ) as RemoteCollection<T>;
  }
  return Object.freeze(collection) as CollectionResource<T, TAccess>;
}

function createDocument<T extends object, TAccess extends ResourceAccess>(
  resource: BoundDocumentDefinition<T, TAccess>,
  backend: DataBackend,
  options: ResourceViewOptions = {},
): DocumentResource<T, TAccess> {
  const remote = options.remote ?? false;
  const ref = createRef<T>(backend.identity, resource.path);
  const document: DocumentReader<T, TAccess>
    & Partial<DocumentCreator<T, TAccess>>
    & Partial<DocumentRemoteReader<T>> = {
    async get() {
      return readItem(resource, backend, ref, !remote);
    },
  };
  if (!remote && resource.access.create === true) {
    document.getOrCreate = async (input) => {
      const existing = await readItem(resource, backend, ref, true);
      if (existing.isPresent()) return existing;
      if (existing.state === State.Unavailable) {
        throw new ItemUnavailableError(ref);
      }
      if (existing.state === State.Deleted) {
        throw new DeletedError(ref);
      }
      const { stored } = currentStoredValue(resource.schema, input);
      const record = await backend.write(ref, stored);
      return presentItem(resource, backend, ref, record, true);
    };
  }
  if (!remote && resource.access.read === Read.AnyIdentity) {
    document.for = identity => createDocument(
      resource,
      backend.for(identity),
      { remote: true },
    ) as RemoteDocument<T>;
  }
  return Object.freeze(document) as DocumentResource<T, TAccess>;
}

function createAppInstance<TData extends AppDataDefinitions>(
  data: BoundAppData<TData>,
  backend: DataBackend,
): AppInstance<TData> {
  return Object.fromEntries(Object.entries(data).map(([name, resource]) => [
    name,
    resource[resourceDefinition] === ResourceKind.Collection
      ? createCollection(resource, backend)
      : createDocument(resource, backend),
  ])) as AppInstance<TData>;
}

function createTestApp<TData extends AppDataDefinitions>(
  data: BoundAppData<TData>,
  options: AppTestOptions = {},
  state: TestWorldState = createTestWorldState(),
): AppInstance<TData> {
  return createAppInstance(
    data,
    createTestBackend(state, options.identity ?? "test.jolt"),
  );
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
    const requirements = Object.freeze(Object.entries(data).map(([resourceName, resource]) => (
      Object.freeze({
        resource: resourceName,
        kind: resource[resourceDefinition],
        access: resource.access,
      })
    )));
    const grants = Object.freeze(Object.entries(data).map(([resourceName, resource]) => (
      Object.freeze({
        resource: resourceName,
        path: resource[resourceDefinition] === ResourceKind.Collection
          ? `${resource.path}/*`
          : resource.path,
        access: resource.access,
      })
    )));
    const accessPlan = Object.freeze({ requirements, grants });
    return {
      id: options.id,
      name: options.name,
      namespace: options.namespace,
      data,
      accessPlan,
      connect: async connectOptions => createAppInstance(
        data,
        createConnectedBackend(connectOptions),
      ),
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

function migratedStoredValue<T extends object>(
  schemaClass: SchemaClass<T>,
  stored: StoredSchemaValue,
): { readonly stored: StoredSchemaValue; readonly value: T } {
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
    return { stored, value: parse(schemaClass, stored.value) };
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
    return {
      stored: { version: options.version, value },
      value: parse(schemaClass, value),
    };
  } catch (cause) {
    throw new SchemaMigrationError(
      stored.version,
      options.version,
      `Migrated value does not match schema version ${options.version}`,
      { cause },
    );
  }
}

function migrate<T extends object>(
  schemaClass: SchemaClass<T>,
  stored: StoredSchemaValue,
): T {
  return migratedStoredValue(schemaClass, stored).value;
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
