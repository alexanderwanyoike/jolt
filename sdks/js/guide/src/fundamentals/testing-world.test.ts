import { describe, expect, it } from "vitest";
import { Feed } from "./subscriptions";

describe("two identities", () => {
  it("shares deterministic public data between Alice and Bob", async () => {
    const world = Feed.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    const post = await alice.posts.create({
      text: "Hello Bob!",
      postedAt: new Date("2026-08-31T12:00:00.000Z"),
    });

    const fromAlice = await bob.posts.for("alice.jolt").get(post.ref);

    expect(fromAlice.isPresent()).toBe(true);
    if (!fromAlice.isPresent()) throw new Error("Expected Alice's post");
    expect(fromAlice.value.text).toBe("Hello Bob!");
    expect("update" in fromAlice).toBe(false);
  });
});
