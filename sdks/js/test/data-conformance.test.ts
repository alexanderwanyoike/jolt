import { describe, expect, it } from "vitest";

import {
  App,
  Collection,
  DeleteConflict,
  Field,
  Read,
  Schema,
  State,
  UpdateConflict,
} from "jolt-sdk/data";
import { createFakeJolt } from "jolt-sdk/testing";

@Schema({ version: 1 })
class Post {
  @Field.string()
  text!: string;

  @Field.dateTime()
  postedAt!: Date;
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

const Chirp = App.create({
  id: "chirp.example",
  name: "Chirp",
  namespace: "chirp",
  data: { posts: Posts },
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
});
