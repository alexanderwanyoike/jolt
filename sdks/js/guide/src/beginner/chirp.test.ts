import { describe, expect, it } from "vitest";
import { State } from "jolt-sdk/data";

import { Chirp, Post } from "./chirp";
import { follow } from "./following";
import { getProfiles, saveNickname } from "./profiles";
import { Timeline } from "./timeline";

describe("beginner Chirp Data SDK example", () => {
  it("persists the identities Alice follows", async () => {
    const alice = Chirp.test({ identity: "alice.jolt" });

    const following = await follow(alice, "bob.jolt");

    expect(following.value.identities).toEqual(["bob.jolt"]);
    expect((await alice.following.get()).isPresent()).toBe(true);
  });

  it("shares a nickname without hiding its canonical Jolt identity", async () => {
    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");

    await saveNickname(alice, "Alice");
    await saveNickname(alice, "Alice W.");
    const profiles = await getProfiles(bob, ["alice.jolt"]);

    expect(profiles.get("alice.jolt")).toEqual({
      identity: "alice.jolt",
      nickname: "Alice W.",
    });
  });

  it("keeps followed identities readable when they have no nickname", async () => {
    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    await follow(alice, "bob.jolt");

    const profiles = await getProfiles(alice, ["bob.jolt"]);

    expect(profiles.get("bob.jolt")).toEqual({ identity: "bob.jolt" });
  });

  it("shows Alice's new post in Bob's open timeline", async () => {
    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    const timeline = await Timeline.open(bob.posts, ["alice.jolt"]);
    const changed = new Promise<void>((resolve) => {
      timeline.subscribe((snapshot) => {
        if (snapshot.posts.some(post => post.value.text === "Hello, Bob!")) resolve();
      });
    });

    await alice.posts.create({
      text: "Hello, Bob!",
      postedAt: new Date("2026-08-29T09:00:00.000Z"),
    });
    await changed;

    expect(timeline.getSnapshot().posts[0]?.value.text).toBe("Hello, Bob!");
    await timeline.close();
  });

  it("creates, edits, deletes, and restores Alice's post", async () => {
    const chirp = Chirp.test({ identity: "alice.jolt" });
    const postedAt = new Date("2026-08-28T12:00:00.000Z");

    const created = await chirp.posts.create({ text: "Hello!", postedAt });
    const updated = await created.update({ text: "Hello, everyone!" });
    const deleted = await updated.delete();
    const restored = await deleted.restore({ text: updated.value.text, postedAt });

    expect(restored.state).toBe(State.Present);
    expect(restored.value).toBeInstanceOf(Post);
    expect(restored.value).toEqual({
      text: "Hello, everyone!",
      postedAt,
    });
  });

  it("loads Alice's existing posts after Bob follows her", async () => {
    const world = Chirp.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    await alice.posts.create({
      text: "First chirp",
      postedAt: new Date("2026-08-29T08:00:00.000Z"),
    });
    await alice.posts.create({
      text: "Second chirp",
      postedAt: new Date("2026-08-29T09:00:00.000Z"),
    });
    const following = await follow(bob, "alice.jolt");

    const timeline = await Timeline.open(bob.posts, [
      bob.identity,
      ...following.value.identities,
    ]);

    expect(timeline.getSnapshot().posts.map(post => post.value.text)).toEqual([
      "Second chirp",
      "First chirp",
    ]);
    await timeline.close();
  });
});
