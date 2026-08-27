import { describe, expect, expectTypeOf, it } from "vitest";

import {
  AccessRevokedError,
  App,
  Collection,
  ConflictError,
  type DeletedItem,
  DeletedError,
  DeleteConflict,
  Document,
  Field,
  ItemUnavailableError,
  Migrations,
  Read,
  Schema,
  SchemaValidationError,
  State,
  UpdateConflict,
} from "jolt-sdk/data";
import { JoltApiError, JoltTransportError } from "jolt-sdk";
import { createFakeJolt } from "jolt-sdk/testing";

@Schema({ version: 1 })
class PostMetadata {
  @Field.string()
  mood!: string;
}

@Schema({ version: 1 })
class Post {
  @Field.string()
  text!: string;

  @Field.dateTime()
  postedAt!: Date;

  @Field.string({ optional: true })
  subtitle?: string;

  @Field.schema(PostMetadata, { optional: true })
  metadata?: PostMetadata;
}

@Schema({ version: 1 })
class FollowList {
  @Field.array(Field.identity)
  identities!: string[];
}

function revokedSessionError(): JoltApiError {
  return new JoltApiError("App session is invalid or revoked", {
    status: 401,
    code: "app_session_unauthorized",
  });
}

const Posts = Collection.create(Post, {
  access: {
    read: Read.AnyIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.DeleteWins,
  },
});

const Follows = Document.create(FollowList, {
  access: {
    read: Read.AnyIdentity,
    create: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.DeleteWins,
  },
});

const Chirp = App.create({
  id: "chirp.example",
  name: "Chirp",
  namespace: "chirp",
  data: { posts: Posts, follows: Follows },
});

const implementations = [
  {
    name: "deterministic",
    connect: async () => Chirp.test({ identity: "alice.jolt" }),
  },
  {
    name: "client-backed",
    connect: async () => {
      const jolt = createFakeJolt("alice.jolt");
      return Chirp.connect({ identity: jolt.identity, client: jolt.client });
    },
  },
] as const;

describe.each(implementations)("Data SDK $name conformance", ({ connect }) => {
  it("creates and reads a typed Collection Item through one stable Ref", async () => {
    const chirp = await connect();
    const postedAt = new Date("2026-08-26T20:00:00.000Z");

    const created = await chirp.posts.create({ text: "Hello!", postedAt });
    const read = await chirp.posts.get(created.ref);

    expect(created.state).toBe(State.Present);
    expect(created.ref).toMatchObject({
      identity: "alice.jolt",
      path: expect.stringMatching(/^\/chirp\/posts\/jlt_/),
    });
    expect(read.ref).toEqual(created.ref);
    expect(read.isPresent()).toBe(true);
    if (!read.isPresent()) throw new Error("expected a present post");
    expect(read.value).toBeInstanceOf(Post);
    expect(read.value).toEqual({ text: "Hello!", postedAt });
    expect(read.value.postedAt).toBeInstanceOf(Date);
  });

  it("creates and reads a typed Document through its one stable Ref", async () => {
    const chirp = await connect();

    const created = await chirp.follows.getOrCreate({ identities: ["bob.jolt"] });
    const read = await chirp.follows.get();

    expect(created.ref).toEqual({
      identity: "alice.jolt",
      path: "/chirp/follows",
    });
    expect(read.ref).toEqual(created.ref);
    expect(read.isPresent()).toBe(true);
    if (!read.isPresent()) throw new Error("expected a present follow list");
    expect(read.value).toBeInstanceOf(FollowList);
    expect(read.value.identities).toEqual(["bob.jolt"]);
  });

  it("updates immutable snapshots and rejects a second mutation from a stale revision", async () => {
    const chirp = await connect();
    const postedAt = new Date("2026-08-26T20:00:00.000Z");
    const replacementDate = new Date("2026-08-26T21:00:00.000Z");
    const created = await chirp.posts.create({ text: "Original", postedAt });

    const updated = await created.update({ text: "Edited" });

    expect(created.value).toEqual({ text: "Original", postedAt });
    expect(updated.ref).toEqual(created.ref);
    expect(updated.value).toEqual({ text: "Edited", postedAt });

    const replaced = await updated.replace({
      text: "Replacement",
      postedAt: replacementDate,
    });
    expect(replaced.ref).toEqual(created.ref);
    expect(replaced.value).toEqual({
      text: "Replacement",
      postedAt: replacementDate,
    });

    const first = await chirp.posts.get(created.ref);
    const stale = await chirp.posts.get(created.ref);
    if (!first.isPresent() || !stale.isPresent()) {
      throw new Error("expected present post snapshots");
    }
    await expect(first.update(null as never)).rejects.toBeInstanceOf(
      SchemaValidationError,
    );
    await expect(
      first.update({ text: 42 as never }),
    ).rejects.toBeInstanceOf(SchemaValidationError);
    const winner = await first.update({ text: "Winner" });
    await expect(stale.update({ text: "Stale" })).rejects.toBeInstanceOf(
      ConflictError,
    );
    const current = await chirp.posts.get(created.ref);
    expect(current).toMatchObject({
      state: State.Present,
      ref: winner.ref,
      value: { text: "Winner" },
    });
  });

  it("deletes a present Item into a Deleted Item at the same Ref", async () => {
    const chirp = await connect();
    const created = await chirp.posts.create({
      text: "Goodbye",
      postedAt: new Date("2026-08-27T08:00:00.000Z"),
    });

    const first = await chirp.posts.get(created.ref);
    const stale = await chirp.posts.get(created.ref);
    if (!first.isPresent() || !stale.isPresent()) {
      throw new Error("expected present post snapshots");
    }
    const deleted = await first.delete();
    await expect(stale.delete()).rejects.toBeInstanceOf(ConflictError);
    const read = await chirp.posts.get(created.ref);

    expect(deleted.state).toBe(State.Deleted);
    expect(deleted.ref).toEqual(created.ref);
    expect(deleted.isPresent()).toBe(false);
    expect(deleted.isDeleted()).toBe(true);
    expect(read.state).toBe(State.Deleted);
    expect(read.ref).toEqual(created.ref);
    expect(created.state).toBe(State.Present);
    expect(created.value.text).toBe("Goodbye");
  });

  it("restores a Deleted Item with a schema-valid value at the same Ref", async () => {
    const chirp = await connect();
    const created = await chirp.posts.create({
      text: "Before deletion",
      postedAt: new Date("2026-08-27T08:00:00.000Z"),
    });
    const deleted = await created.delete();

    await expect(deleted.restore({
      text: 42 as never,
      postedAt: new Date("2026-08-27T09:00:00.000Z"),
    })).rejects.toBeInstanceOf(SchemaValidationError);
    const restored = await deleted.restore({
      text: "Restored",
      postedAt: new Date("2026-08-27T09:00:00.000Z"),
    });

    expect(restored.state).toBe(State.Present);
    expect(restored.ref).toEqual(created.ref);
    expect(restored.value).toEqual({
      text: "Restored",
      postedAt: new Date("2026-08-27T09:00:00.000Z"),
    });
    expect(deleted.state).toBe(State.Deleted);
    await expect(deleted.restore({
      text: "Stale restore",
      postedAt: new Date("2026-08-27T10:00:00.000Z"),
    })).rejects.toBeInstanceOf(ConflictError);
  });
});

const remoteDeleteImplementations = [
  {
    name: "deterministic",
    observeAfterDelete: async () => {
      const world = Chirp.testWorld();
      const alice = world.as("alice.jolt");
      const bob = world.as("bob.jolt");
      const created = await alice.posts.create({
        text: "Remote goodbye",
        postedAt: new Date("2026-08-27T08:00:00.000Z"),
      });
      await created.delete();
      return bob.posts.for("alice.jolt").get(created.ref);
    },
  },
  {
    name: "client-backed",
    observeAfterDelete: async () => {
      const jolt = createFakeJolt("alice.jolt");
      const alice = await Chirp.connect({ identity: jolt.identity, client: jolt.client });
      const created = await alice.posts.create({
        text: "Remote goodbye",
        postedAt: new Date("2026-08-27T08:00:00.000Z"),
      });
      await created.delete();
      const bob = await Chirp.connect({ identity: "bob.jolt", client: jolt.client });
      return bob.posts.for("alice.jolt").get(created.ref);
    },
  },
] as const;

describe.each(remoteDeleteImplementations)(
  "Data SDK $name remote delete conformance",
  ({ observeAfterDelete }) => {
    it("observes a deleted remote Item instead of its old immutable content", async () => {
      const observed = await observeAfterDelete();

      expect(observed.state).toBe(State.Deleted);
      expect(observed.isDeleted()).toBe(true);
    });
  },
);

describe("Data SDK client-backed content validation", () => {
  it("creates from the publish revision without a follow-up record read", async () => {
    const jolt = createFakeJolt("alice.jolt");
    let recordReads = 0;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        async publishJson(path, body) {
          return {
            ...await jolt.client.publishJson(path, body),
            revision: "revision_0",
          };
        },
        read: jolt.client.read,
        readContent: jolt.client.readContent,
        resolve: jolt.client.resolve,
        updateRecord: jolt.client.updateRecord,
        async readRecord() {
          recordReads += 1;
          throw new Error("create must not read back a successful publish");
        },
      },
    });

    const created = await chirp.posts.create({
      text: "Hello!",
      postedAt: new Date("2026-08-27T00:00:00.000Z"),
    });

    expect(created.state).toBe(State.Present);
    expect(recordReads).toBe(0);
  });

  it("falls back to a strict record read when an older publish omits revision", async () => {
    const jolt = createFakeJolt("alice.jolt");
    let recordReads = 0;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        async publishJson(path, body) {
          const { revision: _, ...published } = await jolt.client.publishJson(path, body);
          return published;
        },
        read: jolt.client.read,
        readContent: jolt.client.readContent,
        resolve: jolt.client.resolve,
        updateRecord: jolt.client.updateRecord,
        async readRecord(ref) {
          recordReads += 1;
          return jolt.client.readRecord(ref);
        },
      },
    });

    const created = await chirp.posts.create({
      text: "Legacy host",
      postedAt: new Date("2026-08-27T00:00:00.000Z"),
    });

    expect(created.state).toBe(State.Present);
    expect(recordReads).toBe(1);
  });

  it("updates migrated storage without retaining renamed legacy fields", async () => {
    const migrations = Migrations.create()
      .to(2, value => Migrations.rename(value, { message: "text" }));

    @Schema({ version: 2, migrations })
    class MigratedPost {
      @Field.string()
      text!: string;
    }

    const MigratedPosts = Collection.create(MigratedPost, {
      access: {
        read: Read.OwnIdentity,
        update: true,
      },
      conflicts: {
        update: UpdateConflict.LastWriteWins,
        delete: DeleteConflict.DeleteWins,
      },
    });
    const MigratingApp = App.create({
      id: "migration.example",
      name: "Migration example",
      namespace: "migration",
      data: { posts: MigratedPosts },
    });
    const jolt = createFakeJolt("alice.jolt");
    const path = "/migration/posts/jlt_legacy";
    await jolt.client.publishJson(path, {
      version: 1,
      value: { message: "Original", futureField: "preserved" },
    });
    const app = await MigratingApp.connect({
      identity: jolt.identity,
      client: jolt.client,
    });
    const item = await app.posts.get({ identity: jolt.identity, path });
    if (!item.isPresent()) throw new Error("expected a migrated post");

    await item.update({ text: "Edited" });

    const stored = await jolt.client.read(
      { identity: jolt.identity, path },
      value => value as { version: number; value: Record<string, unknown> },
    );
    expect(stored?.value).toEqual({
      version: 2,
      value: { text: "Edited", futureField: "preserved" },
    });
    expect(stored?.value.value).not.toHaveProperty("message");
  });

  it("preserves unknown stored fields across a shallow update", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const path = "/chirp/posts/jlt_future";
    await jolt.client.publishJson(path, {
      version: 1,
      value: {
        text: "Original",
        postedAt: "2026-08-26T20:00:00.000Z",
        subtitle: "remove me",
        metadata: { mood: "happy", futureNested: true },
        futureField: { kept: true },
      },
    });
    const chirp = await Chirp.connect({ identity: jolt.identity, client: jolt.client });
    const item = await chirp.posts.get({ identity: jolt.identity, path });
    if (!item.isPresent()) throw new Error("expected a present post");

    await item.update({ text: "Edited", subtitle: undefined });

    const stored = await jolt.client.read(
      { identity: jolt.identity, path },
      value => value as { value: Record<string, unknown> },
    );
    expect(stored?.value.value).toMatchObject({
      text: "Edited",
      metadata: { mood: "happy", futureNested: true },
      futureField: { kept: true },
    });
    expect(stored?.value.value).not.toHaveProperty("subtitle");
  });

  it("rejects present content without a schema envelope instead of treating it as Missing", async () => {
    const jolt = createFakeJolt("alice.jolt");
    await jolt.client.publishJson("/chirp/follows", { identities: ["bob.jolt"] });
    const chirp = await Chirp.connect({ identity: jolt.identity, client: jolt.client });

    await expect(chirp.follows.get()).rejects.toBeInstanceOf(SchemaValidationError);
    await expect(
      chirp.follows.getOrCreate({ identities: ["mallory.jolt"] }),
    ).rejects.toBeInstanceOf(SchemaValidationError);
    await expect(chirp.follows.get()).rejects.toBeInstanceOf(SchemaValidationError);
  });

  it("rejects invalid present bytes instead of treating them as Unavailable", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        publishJson: jolt.client.publishJson,
        read: jolt.client.read,
        readContent: jolt.client.readContent,
        resolve: jolt.client.resolve,
        updateRecord: jolt.client.updateRecord,
        async readRecord(ref) {
          return {
            state: "present",
            ref,
            contentId: "cid_invalid",
            revision: "revision_invalid",
            bytes: [255],
          };
        },
      },
    });

    await expect(chirp.follows.get()).rejects.toBeInstanceOf(SchemaValidationError);
  });

  it("returns an Unavailable Item when strict record state cannot be read", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const unavailable = new JoltTransportError("daemon unavailable");
    let publishes = 0;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        async publishJson(...args) {
          publishes += 1;
          return jolt.client.publishJson(...args);
        },
        read: jolt.client.read,
        readContent: jolt.client.readContent,
        resolve: jolt.client.resolve,
        updateRecord: jolt.client.updateRecord,
        async readRecord() {
          throw unavailable;
        },
      },
    });

    const item = await chirp.follows.get();

    expect(item.state).toBe(State.Unavailable);
    expect(item.isPresent()).toBe(false);
    expect(item.isDeleted()).toBe(false);
    await expect(
      chirp.follows.getOrCreate({ identities: ["bob.jolt"] }),
    ).rejects.toBeInstanceOf(ItemUnavailableError);
    expect(publishes).toBe(0);
  });

  it("returns a Deleted Item for a verified local Tombstone", async () => {
    const jolt = createFakeJolt("alice.jolt");
    let publishes = 0;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        async publishJson(...args) {
          publishes += 1;
          return jolt.client.publishJson(...args);
        },
        read: jolt.client.read,
        readContent: jolt.client.readContent,
        resolve: jolt.client.resolve,
        updateRecord: jolt.client.updateRecord,
        async readRecord(ref) {
          return {
            state: "deleted",
            ref,
            revision: "revision_tombstone",
          };
        },
      },
    });

    const item = await chirp.follows.get();

    expect(item.state).toBe(State.Deleted);
    expect(item.ref).toEqual({
      identity: "alice.jolt",
      path: "/chirp/follows",
    });
    expect(item.isPresent()).toBe(false);
    expect(item.isDeleted()).toBe(true);
    if (item.isDeleted()) {
      expectTypeOf(item).toMatchTypeOf<DeletedItem<FollowList>>();
    }
    await expect(
      chirp.follows.getOrCreate({ identities: ["bob.jolt"] }),
    ).rejects.toBeInstanceOf(DeletedError);
    expect(publishes).toBe(0);
  });

  it("maps a daemon-classified content fetch failure to Unavailable", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        publishJson: jolt.client.publishJson,
        read: jolt.client.read,
        readContent: jolt.client.readContent,
        resolve: jolt.client.resolve,
        updateRecord: jolt.client.updateRecord,
        async readRecord() {
          throw new JoltApiError("No content provider", {
            status: 404,
            code: "content_provider_not_found",
          });
        },
      },
    });

    await expect(chirp.follows.get()).resolves.toMatchObject({
      state: State.Unavailable,
    });
  });

  it("does not mistake a remote identity for the daemon's authoritative local state", async () => {
    const jolt = createFakeJolt("alice.jolt");
    let localRecordReads = 0;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        publishJson: jolt.client.publishJson,
        readContent: jolt.client.readContent,
        updateRecord: jolt.client.updateRecord,
        async readRecord(ref) {
          localRecordReads += 1;
          return { state: "missing", ref };
        },
        async read() {
          return null;
        },
      },
    });

    const item = await chirp.posts.for("bob.jolt").get({
      identity: "bob.jolt",
      path: "/chirp/posts/jlt_remote",
    });

    expect(item.state).toBe(State.Unavailable);
    expect(localRecordReads).toBe(0);
  });

  it("maps a verified remote Tombstone to a Deleted Item", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        publishJson: jolt.client.publishJson,
        readContent: jolt.client.readContent,
        readRecord: jolt.client.readRecord,
        updateRecord: jolt.client.updateRecord,
        async read() {
          return null;
        },
        async resolve() {
          throw new JoltApiError("Path is tombstoned", {
            status: 410,
            code: "path_tombstoned",
          });
        },
      },
    });
    const ref = {
      identity: "bob.jolt",
      path: "/chirp/posts/jlt_deleted",
    };

    const item = await chirp.posts.for("bob.jolt").get(ref);

    expect(item.state).toBe(State.Deleted);
    expect(item.ref).toEqual(ref);
    expect(item.isDeleted()).toBe(true);
  });

  it("maps revoked local reads and creates to AccessRevokedError", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        ...jolt.client,
        async publishJson() {
          throw revokedSessionError();
        },
        async readRecord() {
          throw revokedSessionError();
        },
      },
    });

    await expect(chirp.follows.get()).rejects.toBeInstanceOf(AccessRevokedError);
    await expect(chirp.posts.create({
      text: "Denied",
      postedAt: new Date("2026-08-27T09:00:00.000Z"),
    })).rejects.toBeInstanceOf(AccessRevokedError);
  });

  it("does not misclassify revoked remote reads as Unavailable", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        ...jolt.client,
        async resolve() {
          throw revokedSessionError();
        },
      },
    });

    await expect(chirp.posts.for("bob.jolt").get({
      identity: "bob.jolt",
      path: "/chirp/posts/jlt_revoked",
    })).rejects.toBeInstanceOf(AccessRevokedError);

    const revokedDuringFetch = await Chirp.connect({
      identity: jolt.identity,
      client: {
        ...jolt.client,
        async resolve(ref) {
          return { ref, contentId: "cid_revoked_fetch", latestSequence: 1 };
        },
        async readContent() {
          throw revokedSessionError();
        },
      },
    });
    await expect(revokedDuringFetch.posts.for("bob.jolt").get({
      identity: "bob.jolt",
      path: "/chirp/posts/jlt_revoked_fetch",
    })).rejects.toBeInstanceOf(AccessRevokedError);
  });

  it("maps revoked update, delete, and restore mutations to AccessRevokedError", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const seeded = await Chirp.connect({ identity: jolt.identity, client: jolt.client });
    const updateRef = (await seeded.posts.create({
      text: "Update me",
      postedAt: new Date("2026-08-27T09:00:00.000Z"),
    })).ref;
    const deleteRef = (await seeded.posts.create({
      text: "Delete me",
      postedAt: new Date("2026-08-27T09:01:00.000Z"),
    })).ref;
    const deletedRef = (await (await seeded.posts.create({
      text: "Restore me",
      postedAt: new Date("2026-08-27T09:02:00.000Z"),
    })).delete()).ref;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        ...jolt.client,
        async updateRecord() {
          throw revokedSessionError();
        },
        async deleteRecord() {
          throw revokedSessionError();
        },
        async restoreRecord() {
          throw revokedSessionError();
        },
      },
    });
    const updateItem = await chirp.posts.get(updateRef);
    const deleteItem = await chirp.posts.get(deleteRef);
    const restoreItem = await chirp.posts.get(deletedRef);
    if (!updateItem.isPresent() || !deleteItem.isPresent() || !restoreItem.isDeleted()) {
      throw new Error("expected seeded mutation states");
    }

    await expect(updateItem.update({ text: "Denied" }))
      .rejects.toBeInstanceOf(AccessRevokedError);
    await expect(deleteItem.delete()).rejects.toBeInstanceOf(AccessRevokedError);
    await expect(restoreItem.restore({
      text: "Denied",
      postedAt: new Date("2026-08-27T09:03:00.000Z"),
    })).rejects.toBeInstanceOf(AccessRevokedError);
  });
});
