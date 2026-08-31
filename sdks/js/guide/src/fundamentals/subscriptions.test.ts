import { describe, expect, it } from "vitest";
import { SubscriptionState } from "jolt-sdk/data";
import { watchPosts } from "./change-stream-usage";
import { openAuthorPosts } from "./subscription-usage";
import { Feed } from "./subscriptions";

describe("Data SDK subscriptions guide", () => {
  it("opens from retained data and receives later changes without polling", async () => {
    const world = Feed.testWorld();
    const alice = world.as("alice.jolt");
    const bob = world.as("bob.jolt");
    await alice.posts.create({
      text: "Already here",
      postedAt: new Date("2026-08-31T12:00:00.000Z"),
    });

    const { subscription, posts } = await openAuthorPosts(bob, "alice.jolt");
    expect(subscription.state).toBe(SubscriptionState.Ready);
    expect(posts.map(post => post.value.text)).toEqual(["Already here"]);

    let initialView!: () => void;
    const initial = new Promise<void>(resolve => { initialView = resolve; });
    let changedView!: () => void;
    const changed = new Promise<void>(resolve => { changedView = resolve; });
    const views: string[][] = [];
    const watcher = watchPosts(subscription, items => {
      views.push(items.map(item => item.value.text));
      if (views.length === 1) initialView();
      if (views.length === 2) changedView();
    }, () => {});

    await initial;
    await alice.posts.create({
      text: "Arrived live",
      postedAt: new Date("2026-08-31T12:01:00.000Z"),
    });
    await changed;

    expect(views).toEqual([
      ["Already here"],
      ["Already here", "Arrived live"],
    ]);
    await watcher.close();
    await subscription.remove();
    expect(subscription.state).toBe(SubscriptionState.Cancelled);
  });
});
