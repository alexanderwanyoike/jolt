import { describe, expect, it } from "vitest";
import { Post, Posts } from "./migrations";

describe("Data SDK migrations guide", () => {
  it("migrates version one data into the current Schema Class", () => {
    const post = Posts.migrate({
      version: 1,
      value: {
        message: "Hello from version one",
        postedAt: "2026-08-31T12:00:00.000Z",
      },
    });

    expect(post).toBeInstanceOf(Post);
    expect(post.text).toBe("Hello from version one");
    expect(post.tags).toEqual([]);
    expect(post.postedAt).toEqual(new Date("2026-08-31T12:00:00.000Z"));
  });
});
