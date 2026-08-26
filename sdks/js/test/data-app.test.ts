import { describe, expect, it } from "vitest";

import {
  App,
  Collection,
  DeleteConflict,
  Field,
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
});
