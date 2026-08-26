import { describe, expect, it } from "vitest";

import {
  App,
  Collection,
  DeleteConflict,
  Document,
  Field,
  Migrations,
  Read,
  Schema,
  State,
  UpdateConflict,
} from "jolt-sdk/data";

describe("Data SDK applications", () => {
  it("binds a Collection to its App namespace and data property name", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
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
      data: {
        posts: Posts,
      },
    });

    expect("path" in Posts).toBe(false);
    expect(Chirp.data.posts.path).toBe("/chirp/posts");
    expect(Chirp.data.posts.schema).toBe(Post);
  });

  it("binds a Document to one stable App path", () => {
    @Schema({ version: 1 })
    class FollowList {
      @Field.array(Field.identity)
      identities!: string[];
    }

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
      data: {
        follows: Follows,
      },
    });

    expect("path" in Follows).toBe(false);
    expect(Chirp.data.follows.path).toBe("/chirp/follows");
    expect(Chirp.data.follows.schema).toBe(FollowList);
  });

  it("rejects invalid derived path segments at App definition time", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.OwnIdentity },
      conflicts: {
        update: UpdateConflict.LastWriteWins,
        delete: DeleteConflict.DeleteWins,
      },
    });

    for (const namespace of ["", "/chirp", "chirp/", "chirp/posts", "my app", ".", "..", "chirp?x", "chirp#x"]) {
      expect(() => App.create({
        id: "chirp.example",
        name: "Chirp",
        namespace,
        data: { posts: Posts },
      })).toThrowError(`App namespace must be one valid path segment: ${namespace}`);
    }

    for (const resourceName of ["", "/posts", "chirp/posts", "my posts", ".", "..", "posts?x", "posts#x"]) {
      expect(() => App.create({
        id: "chirp.example",
        name: "Chirp",
        namespace: "chirp",
        data: { [resourceName]: Posts },
      })).toThrowError(`Resource name must be one valid path segment: ${resourceName}`);
    }
  });

  it("migrates historical values through a Resource definition", () => {
    const migrations = Migrations.create()
      .to(2, value => Migrations.rename(value, { message: "text" }));

    @Schema({ version: 2, migrations })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: {
        read: Read.AnyIdentity,
      },
      conflicts: {
        update: UpdateConflict.LastWriteWins,
        delete: DeleteConflict.DeleteWins,
      },
    });

    const post = Posts.migrate({
      version: 1,
      value: { message: "Hello!" },
    });

    expect(post).toBeInstanceOf(Post);
    expect(post.text).toBe("Hello!");
  });

  it("creates and reads a typed Collection Item through an isolated App test", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
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
    const chirp = Chirp.test({ identity: "alice.jolt" });

    const created = await chirp.posts.create({ text: "Hello!" });

    expect(created.state).toBe(State.Present);
    expect(created.isPresent()).toBe(true);
    expect(created.ref.identity).toBe("alice.jolt");
    expect(created.ref.path).toMatch(/^\/chirp\/posts\/jlt_/);
    expect(created.value).toBeInstanceOf(Post);
    expect(Object.isFrozen(created)).toBe(true);
    expect(Object.isFrozen(created.ref)).toBe(true);
    expect(Object.isFrozen(created.value)).toBe(true);

    const read = await chirp.posts.get(created.ref);
    expect(read.isPresent()).toBe(true);
    if (!read.isPresent()) throw new Error("expected a present post");
    expect(read.value.text).toBe("Hello!");

    const readByState = await chirp.posts.get(created.ref);
    if (readByState.state !== State.Present) throw new Error("expected a present post");
    expect(readByState.value.text).toBe("Hello!");
  });

  it("keeps nested Item values immutable and separate from deterministic state", async () => {
    @Schema({ version: 1 })
    class Author {
      @Field.string()
      displayName!: string;
    }

    @Schema({ version: 1 })
    class Post {
      @Field.array(Field.string)
      tags!: string[];

      @Field.schema(Author)
      author!: Author;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.OwnIdentity, create: true },
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
    const chirp = Chirp.test();
    const created = await chirp.posts.create({
      tags: ["hello"],
      author: { displayName: "Alice" },
    });

    expect(Object.isFrozen(created.value.tags)).toBe(true);
    expect(Object.isFrozen(created.value.author)).toBe(true);
    expect(() => (created.value.tags as string[]).push("mutated")).toThrow(TypeError);
    expect(() => {
      (created.value.author as Author).displayName = "Mallory";
    }).toThrow(TypeError);
    if (false) {
      // @ts-expect-error Nested snapshot arrays are readonly.
      created.value.tags.push("mutated");
      // @ts-expect-error Nested snapshot objects are readonly.
      created.value.author.displayName = "Mallory";
    }

    const { isPresent } = created;
    expect(isPresent()).toBe(true);

    const read = await chirp.posts.get(created.ref);
    if (!read.isPresent()) throw new Error("expected an unchanged post");
    expect(read.value.tags).toEqual(["hello"]);
    expect(read.value.author.displayName).toBe("Alice");
    expect(read.value).not.toBe(created.value);
  });

  it("gives every App.test call fresh state", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: {
        read: Read.OwnIdentity,
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

    const first = Chirp.test({ identity: "alice.jolt" });
    const created = await first.posts.create({ text: "First test" });
    const second = Chirp.test({ identity: "alice.jolt" });

    const missing = await second.posts.get(created.ref);
    expect(missing.state).toBe(State.Missing);
    expect(missing.isPresent()).toBe(false);
    expect(missing.isDeleted()).toBe(false);
  });

  it("omits Collection creation when Resource access does not declare it", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: {
        read: Read.OwnIdentity,
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
    const chirp = Chirp.test();

    expect("create" in chirp.posts).toBe(false);
    expect("for" in chirp.posts).toBe(false);
    if (false) {
      // @ts-expect-error Read-only Resources do not expose create.
      void chirp.posts.create;
      // @ts-expect-error OwnIdentity Resources do not expose remote views.
      void chirp.posts.for;
    }
  });

  it("reads or creates one stable Document Item", async () => {
    @Schema({ version: 1 })
    class FollowList {
      @Field.array(Field.identity)
      identities!: string[];
    }

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
      data: { follows: Follows },
    });
    const chirp = Chirp.test({ identity: "alice.jolt" });

    const missing = await chirp.follows.get();
    expect(missing.state).toBe(State.Missing);

    const created = await chirp.follows.getOrCreate({
      identities: ["bob.jolt"],
    });
    expect(created.state).toBe(State.Present);
    expect(created.ref).toEqual({
      identity: "alice.jolt",
      path: "/chirp/follows",
    });

    const read = await chirp.follows.get();
    if (!read.isPresent()) throw new Error("expected a present follow list");
    expect(read.value.identities).toEqual(["bob.jolt"]);
  });

  it("shares deterministic state across identities through read-only remote views", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
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

    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    const created = await alice.posts.create({ text: "Hello Bob!" });
    await alice.follows.getOrCreate({ identities: ["bob.jolt"] });

    const alicePosts = bob.posts.for("alice.jolt");
    expect("create" in alicePosts).toBe(false);
    if (false) {
      // @ts-expect-error Remote views are always read-only.
      void alicePosts.create;
    }
    const read = await alicePosts.get(created.ref);
    if (!read.isPresent()) throw new Error("expected Alice's post");
    expect(read.value.text).toBe("Hello Bob!");

    const aliceFollows = bob.follows.for("alice.jolt");
    expect("getOrCreate" in aliceFollows).toBe(false);
    if (false) {
      // @ts-expect-error Remote Document views are always read-only.
      void aliceFollows.getOrCreate;
    }
    const followList = await aliceFollows.get();
    if (!followList.isPresent()) throw new Error("expected Alice's follow list");
    expect(followList.value.identities).toEqual(["bob.jolt"]);
  });
});
