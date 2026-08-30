import { describe, expect, it } from "vitest";
import { createFakeJolt } from "jolt-sdk/testing";

import { Chirp } from "./chirp";
import { Timeline } from "./timeline";

type Fake = ReturnType<typeof createFakeJolt>;
type RawSubscriptionChange = Awaited<
  ReturnType<Fake["client"]["nextDataSubscriptionChange"]>
>;
type PendingChange = {
  resolve(change: RawSubscriptionChange): void;
  reject(error: unknown): void;
};

function controlChanges(fake: Fake) {
  const createSubscription = fake.client.createDataSubscription.bind(fake.client);
  const getView = fake.client.getDataSubscriptionView.bind(fake.client);
  const identities = new Map<string, string>();
  const pending = new Map<string, PendingChange>();
  const requested = new Map<string, () => void>();

  const client: typeof fake.client = {
    ...fake.client,
    async createDataSubscription(identity, prefix, options) {
      const subscription = await createSubscription(identity, prefix, options);
      identities.set(subscription.id, identity);
      return subscription;
    },
    async nextDataSubscriptionChange(subscriptionId, cursor, options) {
      const identity = identities.get(subscriptionId)!;
      if (cursor === undefined) {
        const view = await getView(subscriptionId);
        return {
          type: "snapshot",
          cursor: `snapshot:${identity}`,
          records: view.records,
          state: view.source.state,
        };
      }
      return new Promise((resolve, reject) => {
        pending.set(identity, { resolve, reject });
        requested.get(identity)?.();
        options?.signal?.addEventListener(
          "abort",
          () => resolve({ type: "cancelled" }),
          { once: true },
        );
      });
    },
  };

  return {
    client,
    async waitForRequest(identity: string) {
      if (pending.has(identity)) return;
      await new Promise<void>((resolve) => { requested.set(identity, resolve); });
    },
    emit(identity: string, change: RawSubscriptionChange) {
      const waiter = pending.get(identity)!;
      pending.delete(identity);
      waiter.resolve(change);
    },
    fail(identity: string, error: unknown) {
      const waiter = pending.get(identity)!;
      pending.delete(identity);
      waiter.reject(error);
    },
  };
}

function nextSnapshot(timeline: Timeline) {
  return new Promise<void>((resolve) => {
    const unsubscribe = timeline.subscribe(() => {
      unsubscribe();
      resolve();
    });
  });
}

describe("beginner Chirp timeline", () => {
  it("opens from the stream snapshot without a stale view overwriting it", async () => {
    const fake = createFakeJolt("alice.jolt");
    const alice = await Chirp.connect({ identity: "alice.jolt", client: fake.client });
    await alice.posts.create({
      text: "Already here",
      postedAt: new Date("2026-08-29T08:00:00.000Z"),
    });

    const originalView = fake.client.getDataSubscriptionView.bind(fake.client);
    let releaseStaleView!: () => void;
    const staleView = new Promise<void>((resolve) => { releaseStaleView = resolve; });
    let snapshotSent!: () => void;
    const streamSnapshotWasSent = new Promise<void>((resolve) => { snapshotSent = resolve; });
    let sentInitialSnapshot = false;
    const client: typeof fake.client = {
      ...fake.client,
      async getDataSubscriptionView(subscriptionId) {
        await staleView;
        const view = await originalView(subscriptionId);
        return { ...view, records: [] };
      },
      async nextDataSubscriptionChange(subscriptionId, _cursor, options) {
        if (sentInitialSnapshot) {
          return new Promise((resolve) => {
            options?.signal?.addEventListener(
              "abort",
              () => resolve({ type: "cancelled" }),
              { once: true },
            );
          });
        }
        sentInitialSnapshot = true;
        snapshotSent();
        const view = await originalView(subscriptionId);
        expect(view.records).toHaveLength(1);
        return {
          type: "snapshot",
          cursor: "stream_current:0",
          records: view.records,
          state: view.source.state,
        };
      },
    };
    const bob = await Chirp.connect({ identity: "bob.jolt", client });
    const opening = Timeline.open(bob.posts, ["alice.jolt"]);

    await Promise.race([
      streamSnapshotWasSent,
      opening.then(() => { throw new Error("timeline opened before its first snapshot"); }),
    ]);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    releaseStaleView();
    const timeline = await opening;

    expect(timeline.getSnapshot().posts.map(post => post.value.text)).toEqual([
      "Already here",
    ]);
    await timeline.close();
  });

  it("keeps a timeline error visible when another source changes", async () => {
    const fake = createFakeJolt("alice.jolt");
    const controlled = controlChanges(fake);
    const bob = await Chirp.connect({ identity: "bob.jolt", client: controlled.client });
    const timeline = await Timeline.open(bob.posts, ["alice.jolt", "bob.jolt"]);
    await Promise.all([
      controlled.waitForRequest("alice.jolt"),
      controlled.waitForRequest("bob.jolt"),
    ]);
    const failure = new Error("Alice's stream failed");

    const errorPublished = nextSnapshot(timeline);
    controlled.fail("alice.jolt", failure);
    await errorPublished;

    const bobChangePublished = nextSnapshot(timeline);
    controlled.emit("bob.jolt", {
      type: "changed",
      cursor: "changed:bob.jolt",
      records: [],
      removed: [],
    });
    await bobChangePublished;

    expect(timeline.getSnapshot().error).toBe(failure);
    await timeline.close();
  });

  it("removes a person's posts when Jolt revokes their stream", async () => {
    const fake = createFakeJolt("alice.jolt");
    const alice = await Chirp.connect({ identity: "alice.jolt", client: fake.client });
    await alice.posts.create({
      text: "No longer authorized",
      postedAt: new Date("2026-08-29T08:00:00.000Z"),
    });
    const controlled = controlChanges(fake);
    const bob = await Chirp.connect({ identity: "bob.jolt", client: controlled.client });
    const timeline = await Timeline.open(bob.posts, ["alice.jolt"]);
    await controlled.waitForRequest("alice.jolt");
    expect(timeline.getSnapshot().posts).toHaveLength(1);

    controlled.emit("alice.jolt", { type: "revoked" });
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    expect(timeline.getSnapshot().posts).toEqual([]);
    await timeline.close();
  });
});
