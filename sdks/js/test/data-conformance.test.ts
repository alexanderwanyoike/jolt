import { describe, expect, it } from "vitest";

import {
  App,
  Collection,
  DeleteConflict,
  Document,
  Field,
  ItemUnavailableError,
  Read,
  Schema,
  SchemaValidationError,
  State,
  UpdateConflict,
} from "jolt-sdk/data";
import { JoltTransportError } from "jolt-sdk";
import { createFakeJolt } from "jolt-sdk/testing";

@Schema({ version: 1 })
class Post {
  @Field.string()
  text!: string;

  @Field.dateTime()
  postedAt!: Date;
}

@Schema({ version: 1 })
class FollowList {
  @Field.array(Field.identity)
  identities!: string[];
}

const Posts = Collection.create(Post, {
  access: {
    read: Read.AnyIdentity,
    create: true,
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
});

describe("Data SDK client-backed content validation", () => {
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

  it("does not mistake a remote identity for the daemon's authoritative local state", async () => {
    const jolt = createFakeJolt("alice.jolt");
    let localRecordReads = 0;
    const chirp = await Chirp.connect({
      identity: jolt.identity,
      client: {
        publishJson: jolt.client.publishJson,
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
});
