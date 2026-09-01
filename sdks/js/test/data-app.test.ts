import { describe, expect, expectTypeOf, it } from "vitest";

import {
  App,
  ChangeType,
  Collection,
  DeleteConflict,
  type DeletedItem,
  Document,
  Field,
  Migrations,
  Read,
  ResourceKind,
  Schema,
  State,
  Subscription,
  SubscriptionState,
  UpdateConflict,
} from "jolt-sdk/data";
import { createFakeJolt } from "jolt-sdk/testing";

describe("Data SDK applications", () => {
  it("streams one typed Collection change after its initial snapshot", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.AnyIdentity, create: true },
    });
    const Chirp = App.create({
      id: "chirp.example",
      name: "Chirp",
      namespace: "chirp",
      data: { posts: Posts },
    });
    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    const alicePosts = await Subscription.create(bob.posts.for("alice.jolt"));
    const changes = alicePosts.changes();
    const events = changes[Symbol.asyncIterator]();

    const initial = await events.next();
    expect(initial.done).toBe(false);
    expect(initial.value!.type).toBe(ChangeType.Snapshot);
    if (initial.value!.type === ChangeType.Snapshot) {
      expect(initial.value!.items).toEqual([]);
    }

    await alice.posts.create({ text: "Hello from Alice" });

    const changed = await events.next();
    expect(changed.done).toBe(false);
    expect(changed.value!.type).toBe(ChangeType.Changed);
    if (changed.value!.type === ChangeType.Changed) {
      expect(changed.value!.items.map(item => item.value.text)).toEqual([
        "Hello from Alice",
      ]);
      expectTypeOf(changed.value!.items[0]!.value).toEqualTypeOf<Post>();
      expect(changed.value!.removed).toEqual([]);
    }

    await changes.cancel();
  });

  it("wakes a pending change iterator when its local stream is cancelled", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.AnyIdentity, create: true },
    });
    const Chirp = App.create({
      id: "chirp.example",
      name: "Chirp",
      namespace: "chirp",
      data: { posts: Posts },
    });
    const world = Chirp.testWorld();
    const alicePosts = await Subscription.create(
      world.as("bob.jolt").posts.for("alice.jolt"),
    );
    const changes = alicePosts.changes();
    const events = changes[Symbol.asyncIterator]();

    await events.next();
    const pending = events.next();
    await changes.cancel();

    await expect(pending).resolves.toMatchObject({
      done: false,
      value: { type: ChangeType.Cancelled },
    });
    await expect(events.next()).resolves.toEqual({
      done: true,
      value: undefined,
    });
  });

  it("signals an old cursor once and then automatically reopens from a snapshot", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.AnyIdentity, create: true },
    });
    const Chirp = App.create({
      id: "chirp.example",
      name: "Chirp",
      namespace: "chirp",
      data: { posts: Posts },
    });
    const fake = createFakeJolt("alice.jolt");
    await fake.client.publishJson("/chirp/posts/jlt_one", {
      version: 1,
      value: { text: "Hello after restart" },
    });
    const cursors: Array<string | undefined> = [];
    const client: typeof fake.client = {
      ...fake.client,
      async nextDataSubscriptionChange(subscriptionId, cursor) {
        cursors.push(cursor);
        if (cursor !== undefined) return { type: "resyncRequired" };
        const view = await fake.client.getDataSubscriptionView(subscriptionId);
        return {
          type: "snapshot",
          cursor: "stream_fresh:0",
          records: view.records,
          state: view.source.state,
        };
      },
    };
    const bob = await Chirp.connect({ identity: "bob.jolt", client });
    expect(bob.identity).toBe("bob.jolt");
    const subscription = await Subscription.create(
      bob.posts.for("alice.jolt"),
    );
    const events = subscription.changes({ cursor: "stream_old:7" })[
      Symbol.asyncIterator
    ]();

    await expect(events.next()).resolves.toMatchObject({
      value: { type: ChangeType.ResyncRequired },
    });
    const recovered = await events.next();
    expect(recovered.value?.type).toBe(ChangeType.Snapshot);
    if (recovered.value?.type === ChangeType.Snapshot) {
      expect(recovered.value.items.map(item => item.value.text)).toEqual([
        "Hello after restart",
      ]);
    }
    expect(cursors).toEqual(["stream_old:7", undefined]);
  });

  it("silently re-polls an idle transport timeout with the same cursor", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.AnyIdentity, create: true },
    });
    const Chirp = App.create({
      id: "chirp.example",
      name: "Chirp",
      namespace: "chirp",
      data: { posts: Posts },
    });
    const fake = createFakeJolt("alice.jolt");
    const cursors: Array<string | undefined> = [];
    let poll = 0;
    const client: typeof fake.client = {
      ...fake.client,
      async nextDataSubscriptionChange(_subscriptionId, cursor) {
        cursors.push(cursor);
        poll += 1;
        if (poll === 1) return { type: "timeout", cursor: cursor! };
        return {
          type: "snapshot",
          cursor: cursor!,
          records: [],
          state: { status: "loading" },
        };
      },
    };
    const bob = await Chirp.connect({ identity: "bob.jolt", client });
    const subscription = await Subscription.create(bob.posts.for("alice.jolt"));
    const events = subscription.changes({ cursor: "stream_boot:4" })[
      Symbol.asyncIterator
    ]();

    await expect(events.next()).resolves.toMatchObject({
      value: { type: ChangeType.Snapshot, cursor: "stream_boot:4" },
    });
    expect(cursors).toEqual(["stream_boot:4", "stream_boot:4"]);
  });

  it("creates a typed Data Subscription from a remote Collection view", async () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.AnyIdentity, create: true },
    });
    const Chirp = App.create({
      id: "chirp.example",
      name: "Chirp",
      namespace: "chirp",
      data: { posts: Posts },
    });
    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    await alice.posts.create({ text: "Hello" });
    await alice.posts.create({ text: "Still here" });

    const alicePosts = await Subscription.create(
      bob.posts.for("alice.jolt"),
    );
    expect(alicePosts.state).toBe(SubscriptionState.Loading);

    const posts = await alicePosts.get();

    expect(alicePosts.state).toBe(SubscriptionState.Ready);
    expect(posts.map(post => post.value.text)).toEqual(["Hello", "Still here"]);
    expectTypeOf(posts[0]!.value).toEqualTypeOf<Post>();
    expect(Object.isFrozen(posts)).toBe(true);

    await alicePosts.remove();
    expect(alicePosts.state).toBe(SubscriptionState.Cancelled);
    await expect(alicePosts.get()).rejects.toThrow(/cancelled/i);
  });

  it("uses automatic conflict policies unless a Resource overrides them", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    const Posts = Collection.create(Post, {
      access: {
        read: Read.AnyIdentity,
        create: true,
        update: true,
      },
    });

    expect(Posts.conflicts).toEqual({
      update: UpdateConflict.LastWriteWins,
      delete: DeleteConflict.DeleteWins,
    });
    expectTypeOf(Posts.conflicts.update)
      .toEqualTypeOf<typeof UpdateConflict.LastWriteWins>();
    expectTypeOf(Posts.conflicts.delete)
      .toEqualTypeOf<typeof DeleteConflict.DeleteWins>();
  });

  it("merges a declared conflict policy with the remaining automatic default", () => {
    @Schema({ version: 1 })
    class Note {
      @Field.string()
      text!: string;
    }

    const Notes = Collection.create(Note, {
      access: {
        read: Read.OwnIdentity,
        update: true,
      },
      conflicts: {
        update: UpdateConflict.Manual,
      },
    });

    expect(Notes.conflicts).toEqual({
      update: UpdateConflict.Manual,
      delete: DeleteConflict.DeleteWins,
    });
    expectTypeOf(Notes.conflicts.update)
      .toEqualTypeOf<typeof UpdateConflict.Manual>();
    expectTypeOf(Notes.conflicts.delete)
      .toEqualTypeOf<typeof DeleteConflict.DeleteWins>();
  });

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

  it("derives an inspectable requirement and Grant plan from Resource access", () => {
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
      data: { posts: Posts, follows: Follows },
    });

    expect(Chirp.accessPlan.requirements).toEqual([
      {
        resource: "posts",
        kind: ResourceKind.Collection,
        access: Posts.access,
      },
      {
        resource: "follows",
        kind: ResourceKind.Document,
        access: Follows.access,
      },
    ]);
    expect(Chirp.accessPlan.grants).toEqual([
      {
        resource: "posts",
        path: "/chirp/posts/*",
        access: Posts.access,
      },
      {
        resource: "follows",
        path: "/chirp/follows",
        access: Follows.access,
      },
    ]);
    expect(Chirp.accessPlan.subscriptions).toEqual([
      { resource: "posts", path: "/chirp/posts/*" },
    ]);
    expect(Object.isFrozen(Chirp.accessPlan)).toBe(true);
    expect(Object.isFrozen(Chirp.accessPlan.requirements)).toBe(true);
    expect(Object.isFrozen(Chirp.accessPlan.grants)).toBe(true);
    expect(Object.isFrozen(Chirp.accessPlan.subscriptions)).toBe(true);
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

  it("rejects invalid Resource policies at definition time", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    expect(() => Collection.create(Post, {
      access: { read: "any identity" as never },
      conflicts: {
        update: UpdateConflict.LastWriteWins,
        delete: DeleteConflict.DeleteWins,
      },
    })).toThrowError("Resource access read must be Read.OwnIdentity or Read.AnyIdentity");

    expect(() => Collection.create(Post, {
      access: { read: Read.OwnIdentity, create: false as never },
      conflicts: {
        update: UpdateConflict.LastWriteWins,
        delete: DeleteConflict.DeleteWins,
      },
    })).toThrowError("Resource access create must be true when declared");

    expect(() => Collection.create(Post, {
      access: { read: Read.OwnIdentity, udpate: true } as never,
      conflicts: {
        update: UpdateConflict.LastWriteWins,
        delete: DeleteConflict.DeleteWins,
      },
    })).toThrowError("Unknown Resource access operation: udpate");

    expect(() => Collection.create(Post, {
      access: { read: Read.OwnIdentity },
      conflicts: {
        update: "latest" as never,
        delete: DeleteConflict.DeleteWins,
      },
    })).toThrowError("Resource update conflict must be a value from UpdateConflict");
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

    expect(chirp.identity).toBe("alice.jolt");
    expectTypeOf(chirp.identity).toEqualTypeOf<string>();

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

  it("reserves identity for the connected App's local identity", () => {
    @Schema({ version: 1 })
    class Profile {
      @Field.string()
      name!: string;
    }
    const Identity = Document.create(Profile, {
      access: { read: Read.OwnIdentity },
    });

    expect(() => App.create({
      id: "profiles.example",
      name: "Profiles",
      namespace: "profiles",
      data: { identity: Identity },
    })).toThrow("Resource name is reserved: identity");
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

      @Field.dateTime()
      postedAt!: Date;
    }

    const Posts = Collection.create(Post, {
      access: { read: Read.OwnIdentity, create: true, update: true },
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
      postedAt: new Date("2026-08-26T20:00:00.000Z"),
    });

    expect(Object.isFrozen(created.value.tags)).toBe(true);
    expect(Object.isFrozen(created.value.author)).toBe(true);
    expectTypeOf(created.value.postedAt).toEqualTypeOf<Date>();
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

    const updated = await read.update({
      tags: ["replacement"],
      author: { displayName: "Bob" },
    });
    expect(updated.value.tags).toEqual(["replacement"]);
    expect(updated.value.author.displayName).toBe("Bob");
    expect(updated.value.postedAt).toEqual(created.value.postedAt);
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
    expect("update" in created).toBe(false);
    expect("replace" in created).toBe(false);
    expect("delete" in created).toBe(false);
    expectTypeOf(created).not.toHaveProperty("update");
    expectTypeOf(created).not.toHaveProperty("replace");
    expectTypeOf(created).not.toHaveProperty("delete");
    expectTypeOf<DeletedItem<Post, typeof Posts.access>>().not.toHaveProperty("restore");
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
    expectTypeOf(chirp.posts).not.toHaveProperty("create");
    expectTypeOf(chirp.posts).not.toHaveProperty("for");
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
        update: true,
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
    expectTypeOf(alicePosts).not.toHaveProperty("create");
    const read = await alicePosts.get(created.ref);
    if (!read.isPresent()) throw new Error("expected Alice's post");
    expect(read.value.text).toBe("Hello Bob!");
    expect("update" in read).toBe(false);
    expectTypeOf(read).not.toHaveProperty("update");

    const aliceFollows = bob.follows.for("alice.jolt");
    expect("getOrCreate" in aliceFollows).toBe(false);
    expectTypeOf(aliceFollows).not.toHaveProperty("getOrCreate");
    const followList = await aliceFollows.get();
    if (!followList.isPresent()) throw new Error("expected Alice's follow list");
    expect(followList.value.identities).toEqual(["bob.jolt"]);
  });
});
