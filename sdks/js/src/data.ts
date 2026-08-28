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
  static readonly Conflicted = Symbol("JoltDataStateConflicted");
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
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  readonly state: TState;
  readonly ref: Ref<T>;
  isPresent(): this is PresentItem<T, TAccess, TConflicts>;
  isDeleted(): this is DeletedItem<T, TAccess, TConflicts>;
} & (TConflicts extends (
  | { readonly update: typeof UpdateConflict.Manual }
  | { readonly delete: typeof DeleteConflict.Manual }
) ? {
    isConflicted(): this is ConflictItem<T, TAccess, TConflicts>;
  } : object);

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
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = ItemSnapshot<T, typeof State.Present, TAccess, TConflicts> & {
  readonly value: ImmutableValue<T>;
} & (TAccess extends { readonly update: true } ? {
  update(patch: ShallowPatch<T>): Promise<PresentItem<T, TAccess, TConflicts>>;
  replace(value: T): Promise<PresentItem<T, TAccess, TConflicts>>;
} : object) & (TAccess extends { readonly delete: true } ? {
  delete(): Promise<DeletedItem<T, TAccess, TConflicts>>;
} : object);

/** An immutable Item snapshot whose current state is a Tombstone. */
export type DeletedItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = ItemSnapshot<T, typeof State.Deleted, TAccess, TConflicts> & (
  TAccess extends { readonly restore: true } ? {
    restore(value: T): Promise<PresentItem<T, TAccess, TConflicts>>;
  } : object
);

/** An immutable Item snapshot for a logical reference with no observed record. */
export type MissingItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = ItemSnapshot<T, typeof State.Missing, TAccess, TConflicts>;

/** An immutable Item snapshot whose current state cannot be determined. */
export type UnavailableItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = ItemSnapshot<T, typeof State.Unavailable, TAccess, TConflicts>;

/** One immutable signed state retained as an unresolved Manual alternative. */
export type ConflictAlternative<T extends object> =
  | PresentConflictAlternative<T>
  | DeletedConflictAlternative<T>;

/** One content-bearing alternative in a Manual conflict. */
export type PresentConflictAlternative<T extends object> = {
  readonly state: typeof State.Present;
  readonly ref: Ref<T>;
  readonly value: ImmutableValue<T>;
  isPresent(): this is PresentConflictAlternative<T>;
  isDeleted(): this is DeletedConflictAlternative<T>;
};

/** One deleted alternative in a Manual conflict. */
export type DeletedConflictAlternative<T extends object> = {
  readonly state: typeof State.Deleted;
  readonly ref: Ref<T>;
  isPresent(): this is PresentConflictAlternative<T>;
  isDeleted(): this is DeletedConflictAlternative<T>;
};

/** An immutable unresolved state exposed only by a Resource using Manual policy. */
export type ConflictItem<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  readonly state: typeof State.Conflicted;
  readonly ref: Ref<T>;
  readonly alternatives: readonly ConflictAlternative<T>[];
  isPresent(): this is PresentItem<T, TAccess, TConflicts>;
  isDeleted(): this is DeletedItem<T, TAccess, TConflicts>;
  isConflicted(): this is ConflictItem<T, TAccess, TConflicts>;
} & (TAccess extends { readonly update: true } ? {
  choose(
    alternative: PresentConflictAlternative<T>,
  ): Promise<PresentItem<T, TAccess, TConflicts>>;
  resolve(value: T): Promise<PresentItem<T, TAccess, TConflicts>>;
} : object) & (TAccess extends { readonly delete: true } ? {
  choose(
    alternative: DeletedConflictAlternative<T>,
  ): Promise<DeletedItem<T, TAccess, TConflicts>>;
} : object);

/** Any current immutable state of one logical Item. */
export type Item<
  T extends object,
  TAccess extends ResourceAccess = ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> =
  | PresentItem<T, TAccess, TConflicts>
  | DeletedItem<T, TAccess, TConflicts>
  | MissingItem<T, TAccess, TConflicts>
  | UnavailableItem<T, TAccess, TConflicts>
  | (TConflicts extends (
    | { readonly update: typeof UpdateConflict.Manual }
    | { readonly delete: typeof DeleteConflict.Manual }
  )
    ? ConflictItem<T, TAccess, TConflicts>
    : never);

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
export type ResourceOptions<
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  readonly access: TAccess;
  readonly conflicts: TConflicts;
};

/** Shared metadata and migration behavior for an unbound Resource. */
export type ResourceDefinition<
  T extends object,
  TAccess extends ResourceAccess,
  TKind extends ResourceKindValue,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  readonly schema: SchemaClass<T>;
  readonly access: TAccess;
  readonly conflicts: TConflicts;
  readonly migrate: (stored: StoredSchemaValue) => T;
  readonly [resourceDefinition]: TKind;
};

/** An unbound Collection definition created before it belongs to an App. */
export type CollectionDefinition<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = ResourceDefinition<T, TAccess, typeof ResourceKind.Collection, TConflicts>;

/** An unbound Document definition created before it belongs to an App. */
export type DocumentDefinition<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = ResourceDefinition<T, TAccess, typeof ResourceKind.Document, TConflicts>;

/** A Collection definition bound to its canonical App path prefix. */
export type BoundCollectionDefinition<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = CollectionDefinition<T, TAccess, TConflicts> & {
  readonly path: string;
};

/** A Document definition bound to its one canonical App path. */
export type BoundDocumentDefinition<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = DocumentDefinition<T, TAccess, TConflicts> & {
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

function validatedResourceConflicts<TConflicts extends ResourceConflicts>(
  conflicts: TConflicts,
): TConflicts {
  if (conflicts === null || typeof conflicts !== "object" || Array.isArray(conflicts)) {
    throw new TypeError("Resource conflicts must be an object");
  }
  if (!(Object.values(UpdateConflict) as readonly unknown[]).includes(conflicts.update)) {
    throw new TypeError("Resource update conflict must be a value from UpdateConflict");
  }
  if (!(Object.values(DeleteConflict) as readonly unknown[]).includes(conflicts.delete)) {
    throw new TypeError("Resource delete conflict must be a value from DeleteConflict");
  }
  return Object.freeze({ ...conflicts }) as TConflicts;
}

function defineResource<
  T extends object,
  const TAccess extends ResourceAccess,
  const TConflicts extends ResourceConflicts,
  const TKind extends ResourceKindValue,
>(
  kind: TKind,
  schemaClass: SchemaClass<T>,
  options: ResourceOptions<TAccess, TConflicts>,
): ResourceDefinition<T, TAccess, TKind, TConflicts> {
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
  create: <
    T extends object,
    const TAccess extends ResourceAccess,
    const TConflicts extends ResourceConflicts,
  >(
    schemaClass: SchemaClass<T>,
    options: ResourceOptions<TAccess, TConflicts>,
  ): CollectionDefinition<T, TAccess, TConflicts> => (
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
  create: <
    T extends object,
    const TAccess extends ResourceAccess,
    const TConflicts extends ResourceConflicts,
  >(
    schemaClass: SchemaClass<T>,
    options: ResourceOptions<TAccess, TConflicts>,
  ): DocumentDefinition<T, TAccess, TConflicts> => (
    defineResource(ResourceKind.Document, schemaClass, options)
  ),
} as const;

/** Named unbound Resources accepted by App.create. */
export type AppDataDefinitions = Readonly<Record<
  string,
  | CollectionDefinition<object, ResourceAccess, ResourceConflicts>
  | DocumentDefinition<object, ResourceAccess, ResourceConflicts>
>>;

/** App data definitions after canonical paths have been derived. */
export type BoundAppData<TData extends AppDataDefinitions> = {
  readonly [K in keyof TData]: TData[K] extends CollectionDefinition<
    infer TValue,
    infer TAccess,
    infer TConflicts
  > ? BoundCollectionDefinition<TValue, TAccess, TConflicts>
    : TData[K] extends DocumentDefinition<infer TValue, infer TAccess, infer TConflicts>
      ? BoundDocumentDefinition<TValue, TAccess, TConflicts>
      : never;
};

/** Read operations shared by every connected Collection surface. */
export type CollectionReader<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  get(ref: Ref<T>): Promise<Item<T, TAccess, TConflicts>>;
};

/** Collection creation exposed only when declared in Resource access. */
export type CollectionCreator<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  create(value: T): Promise<PresentItem<T, TAccess, TConflicts>>;
};

/** A read-only Collection view bound to another identity. */
export type RemoteCollection<
  T extends object,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = CollectionReader<T, { readonly read: typeof Read.AnyIdentity }, TConflicts>;

/** Remote Collection reads exposed only for AnyIdentity access. */
export type CollectionRemoteReader<
  T extends object,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  for(identity: Identity): RemoteCollection<T, TConflicts>;
};

/** A connected Collection surface derived from its access declaration. */
export type CollectionResource<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = CollectionReader<T, TAccess, TConflicts> & (
  TAccess extends { readonly create: true }
    ? CollectionCreator<T, TAccess, TConflicts>
    : object
) & (
  TAccess["read"] extends typeof Read.AnyIdentity
    ? CollectionRemoteReader<T, TConflicts>
    : object
);

/** Read operations shared by every connected Document surface. */
export type DocumentReader<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  get(): Promise<Item<T, TAccess, TConflicts>>;
};

/** Document creation exposed only when declared in Resource access. */
export type DocumentCreator<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  getOrCreate(value: T): Promise<PresentItem<T, TAccess, TConflicts>>;
};

/** A read-only Document view bound to another identity. */
export type RemoteDocument<
  T extends object,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = DocumentReader<T, { readonly read: typeof Read.AnyIdentity }, TConflicts>;

/** Remote Document reads exposed only for AnyIdentity access. */
export type DocumentRemoteReader<
  T extends object,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = {
  for(identity: Identity): RemoteDocument<T, TConflicts>;
};

/** A connected Document surface derived from its access declaration. */
export type DocumentResource<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts = ResourceConflicts,
> = DocumentReader<T, TAccess, TConflicts> & (
  TAccess extends { readonly create: true }
    ? DocumentCreator<T, TAccess, TConflicts>
    : object
) & (
  TAccess["read"] extends typeof Read.AnyIdentity
    ? DocumentRemoteReader<T, TConflicts>
    : object
);

/** The connected Resource surface generated from one Resource definition. */
export type AppResource<TDefinition> = TDefinition extends CollectionDefinition<
  infer TValue,
  infer TAccess,
  infer TConflicts
> ? CollectionResource<TValue, TAccess, TConflicts>
  : TDefinition extends DocumentDefinition<infer TValue, infer TAccess, infer TConflicts>
    ? DocumentResource<TValue, TAccess, TConflicts>
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
  | "restoreRecord"
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
  /** Returns a view over the world's immediately shared application state. */
  as(identity: Identity): AppInstance<TData>;
  /**
   * Creates one isolated device replica for deterministic offline-branch tests.
   * After synchronization, each Resource applies its declared update and delete
   * conflict policies without relying on device wall clocks.
   */
  device(identity: Identity, deviceId: string): AppInstance<TData>;
  /** Exchanges known histories so every device observes the same branches. */
  sync(): Promise<void>;
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

function falseIsConflicted(): false {
  return false;
}

function trueIsConflicted(): true {
  return true;
}

function usesManualConflict(conflicts: ResourceConflicts): boolean {
  return conflicts.update === UpdateConflict.Manual
    || conflicts.delete === DeleteConflict.Manual;
}

function conflictNeedsManualResolution(
  conflicts: ResourceConflicts,
  conflict: BackendConflictRecord,
): boolean {
  const hasDeleted = conflict.alternatives.some(isBackendDeletedRecord);
  const hasPresent = conflict.alternatives.some(alternative => (
    !isBackendDeletedRecord(alternative)
  ));
  if (hasDeleted && hasPresent) return conflicts.delete === DeleteConflict.Manual;
  if (hasPresent) return conflicts.update === UpdateConflict.Manual;
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

function presentItem<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue, TConflicts>,
  backend: DataBackend,
  ref: Ref<T>,
  record: BackendPresentRecord,
  mutable: boolean,
): PresentItem<T, TAccess, TConflicts> {
  const migrated = migratedStoredValue(resource.schema, record.stored);
  const value = migrated.value;
  const item: Record<string, unknown> = {
    state: State.Present,
    ref,
    value: freezeValue(value),
    isPresent: trueIsPresent,
    isDeleted: falseIsDeleted,
  };
  if (usesManualConflict(resource.conflicts)) {
    item.isConflicted = falseIsConflicted;
  }
  const revision = mutable ? record.revision : null;
  const observedRevisions = mutable ? record.observedRevisions : undefined;
  if (
    resource.access.update === true
    && (revision !== null || observedRevisions !== undefined)
  ) {
    const commit = async (stored: StoredSchemaValue) => {
      const next = observedRevisions === undefined
        ? await backend.update(
          ref,
          stored,
          revision!,
          backend.nextMutationId(),
        )
        : await backend.resolveConflict(
          ref,
          stored,
          observedRevisions,
          backend.nextMutationId(),
        );
      if (isBackendDeletedRecord(next)) throw new ConflictError(ref);
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
  if (
    resource.access.delete === true
    && (revision !== null || observedRevisions !== undefined)
  ) {
    item.delete = async () => {
      const deleted = observedRevisions === undefined
        ? await backend.delete(ref, revision!, backend.nextMutationId())
        : await backend.resolveConflict(
          ref,
          State.Deleted,
          observedRevisions,
          backend.nextMutationId(),
        );
      if (!isBackendDeletedRecord(deleted)) throw new ConflictError(ref);
      return deletedItem(resource, ref, { backend, revision: deleted.revision });
    };
  }
  return Object.freeze(item) as PresentItem<T, TAccess, TConflicts>;
}

function missingItem<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue, TConflicts>,
  ref: Ref<T>,
): MissingItem<T, TAccess, TConflicts> {
  const item: Record<string, unknown> = {
    state: State.Missing,
    ref,
    isPresent: falseIsPresent,
    isDeleted: falseIsDeleted,
  };
  if (usesManualConflict(resource.conflicts)) {
    item.isConflicted = falseIsConflicted;
  }
  return Object.freeze(item) as MissingItem<T, TAccess, TConflicts>;
}

function deletedItem<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue, TConflicts>,
  ref: Ref<T>,
  mutationContext?: {
    readonly backend: DataBackend;
    readonly revision: string;
    readonly observedRevisions?: readonly string[];
  },
): DeletedItem<T, TAccess, TConflicts> {
  const item: Record<string, unknown> = {
    state: State.Deleted,
    ref,
    isPresent: falseIsPresent,
    isDeleted: trueIsDeleted,
  };
  if (usesManualConflict(resource.conflicts)) {
    item.isConflicted = falseIsConflicted;
  }
  if (resource.access.restore === true && mutationContext !== undefined) {
    item.restore = async (input: T) => {
      const current = currentStoredValue(resource.schema, input);
      const restored = mutationContext.observedRevisions === undefined
        ? await mutationContext.backend.restore(
          ref,
          current.stored,
          mutationContext.revision,
          mutationContext.backend.nextMutationId(),
        )
        : await mutationContext.backend.resolveConflict(
          ref,
          current.stored,
          mutationContext.observedRevisions,
          mutationContext.backend.nextMutationId(),
        );
      if (isBackendDeletedRecord(restored)) throw new ConflictError(ref);
      return presentItem(resource, mutationContext.backend, ref, restored, true);
    };
  }
  return Object.freeze(item) as DeletedItem<T, TAccess, TConflicts>;
}

function conflictItem<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue, TConflicts>,
  backend: DataBackend,
  ref: Ref<T>,
  record: BackendConflictRecord,
  mutable: boolean,
): ConflictItem<T, TAccess, TConflicts> {
  const recordsByAlternative = new Map<ConflictAlternative<T>, BackendAlternativeRecord>();
  const alternatives = Object.freeze(record.alternatives.map(alternativeRecord => {
    let alternative: ConflictAlternative<T>;
    if (isBackendDeletedRecord(alternativeRecord)) {
      alternative = Object.freeze({
        state: State.Deleted,
        ref,
        isPresent: falseIsPresent,
        isDeleted: trueIsDeleted,
      }) as DeletedConflictAlternative<T>;
    } else {
      const migrated = migratedStoredValue(resource.schema, alternativeRecord.stored);
      alternative = Object.freeze({
        state: State.Present,
        ref,
        value: freezeValue(migrated.value),
        isPresent: trueIsPresent,
        isDeleted: falseIsDeleted,
      }) as PresentConflictAlternative<T>;
    }
    recordsByAlternative.set(alternative, alternativeRecord);
    return alternative;
  }));
  const item: Record<string, unknown> = {
    state: State.Conflicted,
    ref,
    alternatives,
    isPresent: falseIsPresent,
    isDeleted: falseIsDeleted,
    isConflicted: trueIsConflicted,
  };
  if (mutable && (resource.access.update === true || resource.access.delete === true)) {
    item.choose = async (alternative: ConflictAlternative<T>) => {
      const selected = recordsByAlternative.get(alternative);
      if (selected === undefined) {
        throw new TypeError("Conflict alternative does not belong to this Item");
      }
      if (isBackendDeletedRecord(selected) && resource.access.delete !== true) {
        throw new TypeError("Resource does not allow choosing a deleted alternative");
      }
      if (!isBackendDeletedRecord(selected) && resource.access.update !== true) {
        throw new TypeError("Resource does not allow choosing a present alternative");
      }
      const resolved = await backend.resolveConflict(
        ref,
        isBackendDeletedRecord(selected) ? State.Deleted : selected.stored,
        record.revisions,
        backend.nextMutationId(),
      );
      return isBackendDeletedRecord(resolved)
        ? deletedItem(resource, ref, { backend, revision: resolved.revision })
        : presentItem(resource, backend, ref, resolved, true);
    };
  }
  if (mutable && resource.access.update === true) {
    item.resolve = async (input: T) => {
      const current = currentStoredValue(resource.schema, input);
      const resolved = await backend.resolveConflict(
        ref,
        current.stored,
        record.revisions,
        backend.nextMutationId(),
      );
      if (isBackendDeletedRecord(resolved)) throw new ConflictError(ref);
      return presentItem(resource, backend, ref, resolved, true);
    };
  }
  return Object.freeze(item) as ConflictItem<T, TAccess, TConflicts>;
}

function unavailableItem<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue, TConflicts>,
  ref: Ref<T>,
): UnavailableItem<T, TAccess, TConflicts> {
  const item: Record<string, unknown> = {
    state: State.Unavailable,
    ref,
    isPresent: falseIsPresent,
    isDeleted: falseIsDeleted,
  };
  if (usesManualConflict(resource.conflicts)) {
    item.isConflicted = falseIsConflicted;
  }
  return Object.freeze(item) as UnavailableItem<T, TAccess, TConflicts>;
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
  readonly observedRevisions?: readonly string[];
};

type BackendDeletedRecord = {
  readonly state: typeof State.Deleted;
  readonly revision: string | null;
  readonly observedRevisions?: readonly string[];
};

type BackendAlternativeRecord =
  | (BackendPresentRecord & { readonly revision: string })
  | (BackendDeletedRecord & { readonly revision: string });

type BackendConflictRecord = {
  readonly state: typeof State.Conflicted;
  readonly alternatives: readonly BackendAlternativeRecord[];
  readonly revisions: readonly string[];
  readonly base?: BackendAlternativeRecord;
};

type BackendReadResult =
  | BackendPresentRecord
  | BackendDeletedRecord
  | BackendConflictRecord
  | typeof State.Missing
  | typeof State.Unavailable;

function isBackendDeletedRecord(
  record: BackendPresentRecord | BackendDeletedRecord | BackendConflictRecord,
): record is BackendDeletedRecord {
  return "state" in record && record.state === State.Deleted;
}

function isBackendConflictRecord(
  record: BackendPresentRecord | BackendDeletedRecord | BackendConflictRecord,
): record is BackendConflictRecord {
  return "state" in record && record.state === State.Conflicted;
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
  restore<T extends object>(
    ref: Ref<T>,
    stored: StoredSchemaValue,
    revision: string,
    mutationId: string,
  ): Promise<BackendPresentRecord & { readonly revision: string }>;
  resolveConflict<T extends object>(
    ref: Ref<T>,
    next: StoredSchemaValue | typeof State.Deleted,
    revisions: readonly string[],
    mutationId: string,
  ): Promise<BackendAlternativeRecord>;
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
    async restore(ref, stored, revision) {
      const key = testStoreKey(ref);
      const current = state.store.get(key);
      if (
        current === undefined
        || !isBackendDeletedRecord(current)
        || current.revision !== revision
      ) {
        throw new ConflictError(ref);
      }
      const record = { stored, revision: `revision_${++state.nextRevision}` };
      state.store.set(key, record);
      return record;
    },
    async resolveConflict(ref) {
      throw new ConflictError(ref);
    },
    for: remoteIdentity => createTestBackend(state, remoteIdentity),
  };
}

type TestDeviceBranch = {
  readonly revision: string;
  readonly parents: readonly string[];
  readonly record: BackendAlternativeRecord;
};

type TestDeviceState = {
  readonly id: string;
  readonly history: Map<string, Map<string, TestDeviceBranch>>;
};

type TestDeviceWorldState = {
  readonly devices: Map<string, TestDeviceState>;
  nextId: number;
  nextMutationId: number;
  nextRevision: number;
};

function createTestDeviceWorldState(): TestDeviceWorldState {
  return {
    devices: new Map(),
    nextId: 0,
    nextMutationId: 0,
    nextRevision: 0,
  };
}

function testDeviceHeads(
  device: TestDeviceState,
  key: string,
): readonly TestDeviceBranch[] {
  const branches = device.history.get(key);
  if (branches === undefined) return [];
  const superseded = new Set(
    [...branches.values()].flatMap(branch => branch.parents),
  );
  return [...branches.values()]
    .filter(branch => !superseded.has(branch.revision))
    .sort((left, right) => (
      left.revision < right.revision ? -1 : left.revision > right.revision ? 1 : 0
    ));
}

function testDeviceAncestorRevisions(
  branch: TestDeviceBranch,
  branches: ReadonlyMap<string, TestDeviceBranch>,
): ReadonlySet<string> {
  const ancestors = new Set<string>();
  const pending = [...branch.parents];
  while (pending.length > 0) {
    const revision = pending.pop()!;
    if (ancestors.has(revision)) continue;
    ancestors.add(revision);
    const parent = branches.get(revision);
    if (parent !== undefined) pending.push(...parent.parents);
  }
  return ancestors;
}

function testDeviceCommonAncestor(
  device: TestDeviceState,
  key: string,
  heads: readonly TestDeviceBranch[],
): BackendAlternativeRecord | undefined {
  const branches = device.history.get(key);
  if (branches === undefined || heads.length < 2) return undefined;
  const ancestorSets = heads.map(head => testDeviceAncestorRevisions(head, branches));
  const common = [...ancestorSets[0]!].filter(revision => (
    ancestorSets.slice(1).every(ancestors => ancestors.has(revision))
  ));
  const latest = common.filter(revision => !common.some(other => (
    other !== revision
    && testDeviceAncestorRevisions(branches.get(other)!, branches).has(revision)
  )));
  return latest.length === 1 ? branches.get(latest[0]!)?.record : undefined;
}

function syncTestDevices(world: TestDeviceWorldState): void {
  const merged = new Map<string, Map<string, TestDeviceBranch>>();
  for (const device of world.devices.values()) {
    for (const [key, branches] of device.history) {
      const mergedBranches = merged.get(key) ?? new Map<string, TestDeviceBranch>();
      for (const [revision, branch] of branches) {
        mergedBranches.set(revision, branch);
      }
      merged.set(key, mergedBranches);
    }
  }
  for (const device of world.devices.values()) {
    device.history.clear();
    for (const [key, branches] of merged) {
      device.history.set(key, new Map(branches));
    }
  }
}

function createTestDeviceBackend(
  world: TestDeviceWorldState,
  device: TestDeviceState,
  identity: Identity,
): DataBackend {
  function commit(
    ref: Pick<Ref<object>, "identity" | "path">,
    record: { readonly stored: StoredSchemaValue },
    parents: readonly string[],
  ): BackendPresentRecord & { readonly revision: string };
  function commit(
    ref: Pick<Ref<object>, "identity" | "path">,
    record: { readonly state: typeof State.Deleted },
    parents: readonly string[],
  ): BackendDeletedRecord & { readonly revision: string };
  function commit(
    ref: Pick<Ref<object>, "identity" | "path">,
    record: { readonly stored: StoredSchemaValue }
      | { readonly state: typeof State.Deleted },
    parents: readonly string[],
  ): BackendAlternativeRecord {
    const revision = `${device.id}:${++world.nextRevision}`;
    const next = { ...record, revision } as BackendAlternativeRecord;
    const key = testStoreKey(ref);
    const history = device.history.get(key) ?? new Map<string, TestDeviceBranch>();
    history.set(revision, Object.freeze({
      revision,
      parents: Object.freeze([...parents]),
      record: next,
    }));
    device.history.set(key, history);
    return next;
  }

  function matchingHead(
    ref: Pick<Ref<object>, "identity" | "path">,
    revision: string,
  ): TestDeviceBranch {
    const heads = testDeviceHeads(device, testStoreKey(ref));
    if (heads.length !== 1 || heads[0]?.revision !== revision) {
      throw new ConflictError(ref);
    }
    return heads[0];
  }

  return {
    identity,
    nextId: () => `jlt_${(++world.nextId).toString(36).padStart(12, "0")}`,
    nextMutationId: () => `mut_${(++world.nextMutationId).toString(36).padStart(12, "0")}`,
    async read(ref) {
      const key = testStoreKey(ref);
      const heads = testDeviceHeads(device, key);
      if (heads.length === 0) return State.Missing;
      if (heads.length === 1) return heads[0]!.record;
      return {
        state: State.Conflicted,
        alternatives: Object.freeze(heads.map(head => head.record)),
        revisions: Object.freeze(heads.map(head => head.revision)),
        base: testDeviceCommonAncestor(device, key, heads),
      };
    },
    async write(ref, stored) {
      if (ref.identity !== identity || testDeviceHeads(device, testStoreKey(ref)).length > 0) {
        throw new ConflictError(ref);
      }
      return commit(ref, { stored }, []);
    },
    async update(ref, stored, revision) {
      const current = matchingHead(ref, revision);
      if (ref.identity !== identity || isBackendDeletedRecord(current.record)) {
        throw new ConflictError(ref);
      }
      return commit(ref, { stored }, [revision]);
    },
    async delete(ref, revision) {
      const current = matchingHead(ref, revision);
      if (ref.identity !== identity || isBackendDeletedRecord(current.record)) {
        throw new ConflictError(ref);
      }
      return commit(ref, { state: State.Deleted }, [revision]) as (
        BackendDeletedRecord & { readonly revision: string }
      );
    },
    async restore(ref, stored, revision) {
      const current = matchingHead(ref, revision);
      if (ref.identity !== identity || !isBackendDeletedRecord(current.record)) {
        throw new ConflictError(ref);
      }
      return commit(ref, { stored }, [revision]) as (
        BackendPresentRecord & { readonly revision: string }
      );
    },
    async resolveConflict(ref, next, revisions) {
      const heads = testDeviceHeads(device, testStoreKey(ref));
      const currentRevisions = heads.map(head => head.revision);
      if (
        ref.identity !== identity
        || heads.length < 2
        || currentRevisions.length !== revisions.length
        || currentRevisions.some((revision, index) => revision !== revisions[index])
      ) {
        throw new ConflictError(ref);
      }
      return next === State.Deleted
        ? commit(ref, { state: State.Deleted }, revisions)
        : commit(ref, { stored: next }, revisions);
    },
    for: remoteIdentity => createTestDeviceBackend(world, device, remoteIdentity),
  };
}

async function withConnectedAccess<T>(call: () => Promise<T>): Promise<T> {
  try {
    return await call();
  } catch (error) {
    if (error instanceof JoltApiError && error.code === "app_session_unauthorized") {
      throw new AccessRevokedError({ cause: error });
    }
    throw error;
  }
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
          resolved = await withConnectedAccess(() => options.client.resolve(ref));
        } catch (error) {
          if (error instanceof AccessRevokedError) throw error;
          if (error instanceof JoltApiError && error.code === "path_tombstoned") {
            return { state: State.Deleted, revision: null };
          }
          return State.Unavailable;
        }
        const versioned = await withConnectedAccess(() => options.client.readContent(
          resolved.contentId,
          ref,
          resolved.latestSequence,
          value => ({ value }),
        ));
        if (versioned === null) return State.Unavailable;
        return {
          stored: requireStoredSchemaValue(versioned.value.value),
          revision: null,
        };
      }
      let record;
      try {
        record = await withConnectedAccess(() => options.client.readRecord(ref));
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
      const published = await withConnectedAccess(() => (
        options.client.publishJson(ref.path, stored)
      ));
      if (published.revision !== undefined) {
        return { stored, revision: published.revision };
      }
      const record = await withConnectedAccess(() => options.client.readRecord(ref));
      if (record.state !== "present") {
        throw new ItemUnavailableError(ref);
      }
      return backendRecord(record.bytes, record.revision);
    },
    async update(ref, stored, revision, mutationId) {
      try {
        const record = await withConnectedAccess(() => options.client.updateRecord(
          ref,
          stored,
          { revision, mutationId },
        ));
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
        const record = await withConnectedAccess(() => options.client.deleteRecord(
          ref,
          { revision, mutationId },
        ));
        return { state: State.Deleted, revision: record.revision };
      } catch (error) {
        if (error instanceof JoltApiError && error.code === "record_conflict") {
          throw new ConflictError(ref);
        }
        throw error;
      }
    },
    async restore(ref, stored, revision, mutationId) {
      try {
        const record = await withConnectedAccess(() => options.client.restoreRecord(
          ref,
          stored,
          { revision, mutationId },
        ));
        return backendRecord(record.bytes, record.revision) as BackendPresentRecord & {
          readonly revision: string;
        };
      } catch (error) {
        if (error instanceof JoltApiError && error.code === "record_conflict") {
          throw new ConflictError(ref);
        }
        throw error;
      }
    },
    async resolveConflict(ref) {
      throw new ConflictError(ref);
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

function storedValuesEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (Array.isArray(left) || Array.isArray(right)) {
    return Array.isArray(left)
      && Array.isArray(right)
      && left.length === right.length
      && left.every((value, index) => storedValuesEqual(value, right[index]));
  }
  if (
    left === null
    || right === null
    || typeof left !== "object"
    || typeof right !== "object"
  ) {
    return false;
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord);
  const rightKeys = Object.keys(rightRecord);
  return leftKeys.length === rightKeys.length
    && leftKeys.every(key => (
      Object.prototype.hasOwnProperty.call(rightRecord, key)
      && storedValuesEqual(leftRecord[key], rightRecord[key])
    ));
}

function mergeConcurrentPresentUpdates<T extends object>(
  schemaClass: SchemaClass<T>,
  conflict: BackendConflictRecord,
): BackendPresentRecord | null {
  if (conflict.base === undefined || isBackendDeletedRecord(conflict.base)) return null;
  const alternatives = conflict.alternatives.filter(alternative => (
    !isBackendDeletedRecord(alternative)
  )) as readonly (BackendPresentRecord & { readonly revision: string })[];
  if (alternatives.length === 0) return null;

  const base = migratedStoredValue(schemaClass, conflict.base.stored).stored;
  const baseValue = base.value as Record<string, unknown>;
  const merged = { ...baseValue };
  const changes = new Map<string, { readonly present: boolean; readonly value?: unknown }>();

  for (const alternative of alternatives) {
    const stored = migratedStoredValue(schemaClass, alternative.stored).stored;
    const value = stored.value as Record<string, unknown>;
    const keys = new Set([...Object.keys(baseValue), ...Object.keys(value)]);
    for (const key of keys) {
      const basePresent = Object.prototype.hasOwnProperty.call(baseValue, key);
      const present = Object.prototype.hasOwnProperty.call(value, key);
      if (
        basePresent === present
        && (!present || storedValuesEqual(baseValue[key], value[key]))
      ) {
        continue;
      }
      const next = { present, value: value[key] };
      changes.set(key, next);
    }
  }

  for (const [key, change] of changes) {
    if (change.present) merged[key] = change.value;
    else delete merged[key];
  }
  return {
    stored: { version: base.version, value: merged },
    revision: conflict.revisions.at(-1) ?? null,
    observedRevisions: conflict.revisions,
  };
}

async function readItem<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: ResourceDefinition<T, TAccess, ResourceKindValue, TConflicts>,
  backend: DataBackend,
  ref: Ref<T>,
  mutable: boolean,
): Promise<Item<T, TAccess, TConflicts>> {
  const stored = await backend.read(ref);
  if (stored === State.Missing) return missingItem(resource, ref);
  if (stored === State.Unavailable) return unavailableItem(resource, ref);
  if (isBackendConflictRecord(stored)) {
    if (conflictNeedsManualResolution(resource.conflicts, stored)) {
      return conflictItem(resource, backend, ref, stored, mutable) as Item<
        T,
        TAccess,
        TConflicts
      >;
    }
    const deleted = [...stored.alternatives].reverse().find(isBackendDeletedRecord);
    const hasPresent = stored.alternatives.some(alternative => (
      !isBackendDeletedRecord(alternative)
    ));
    if (
      deleted !== undefined
      && (!hasPresent || resource.conflicts.delete === DeleteConflict.DeleteWins)
    ) {
      return deletedItem(
        resource,
        ref,
        mutable
          ? {
            backend,
            revision: deleted.revision,
            observedRevisions: stored.revisions,
          }
          : undefined,
      );
    }
    const merged = mergeConcurrentPresentUpdates(resource.schema, stored);
    if (merged === null) throw new ConflictError(ref);
    return presentItem(resource, backend, ref, merged, mutable);
  }
  if (isBackendDeletedRecord(stored)) {
    return deletedItem(
      resource,
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

function createCollection<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: BoundCollectionDefinition<T, TAccess, TConflicts>,
  backend: DataBackend,
  options: ResourceViewOptions = {},
): CollectionResource<T, TAccess, TConflicts> {
  const remote = options.remote ?? false;
  const collection: CollectionReader<T, TAccess, TConflicts>
    & Partial<CollectionCreator<T, TAccess, TConflicts>>
    & Partial<CollectionRemoteReader<T, TConflicts>> = {
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
    ) as RemoteCollection<T, TConflicts>;
  }
  return Object.freeze(collection) as CollectionResource<T, TAccess, TConflicts>;
}

function createDocument<
  T extends object,
  TAccess extends ResourceAccess,
  TConflicts extends ResourceConflicts,
>(
  resource: BoundDocumentDefinition<T, TAccess, TConflicts>,
  backend: DataBackend,
  options: ResourceViewOptions = {},
): DocumentResource<T, TAccess, TConflicts> {
  const remote = options.remote ?? false;
  const ref = createRef<T>(backend.identity, resource.path);
  const document: DocumentReader<T, TAccess, TConflicts>
    & Partial<DocumentCreator<T, TAccess, TConflicts>>
    & Partial<DocumentRemoteReader<T, TConflicts>> = {
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
    ) as RemoteDocument<T, TConflicts>;
  }
  return Object.freeze(document) as DocumentResource<T, TAccess, TConflicts>;
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
        const deviceWorld = createTestDeviceWorldState();
        const sharedDevice = { id: "default", history: new Map() };
        deviceWorld.devices.set("default", sharedDevice);
        return Object.freeze({
          as: (identity: Identity) => createAppInstance(
            data,
            createTestDeviceBackend(deviceWorld, sharedDevice, identity),
          ),
          device: (identity: Identity, deviceId: string) => {
            if (deviceId.length === 0) {
              throw new TypeError("Test device ID must not be empty");
            }
            const key = `${identity}\u0000${deviceId}`;
            if (deviceWorld.devices.has(key)) {
              throw new TypeError(`Test device already exists: ${identity}/${deviceId}`);
            }
            const device = { id: deviceId, history: new Map() };
            deviceWorld.devices.set(key, device);
            return createAppInstance(
              data,
              createTestDeviceBackend(deviceWorld, device, identity),
            );
          },
          sync: async () => syncTestDevices(deviceWorld),
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

/** The App Session no longer authorizes connected Data SDK operations. */
export class AccessRevokedError extends Error {
  constructor(options?: ErrorOptions) {
    super("Jolt access was revoked; reconnect and request approval again", options);
    this.name = "AccessRevokedError";
  }
}
