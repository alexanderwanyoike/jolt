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
});
