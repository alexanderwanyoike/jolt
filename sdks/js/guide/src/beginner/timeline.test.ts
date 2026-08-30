import { describe, expect, it } from "vitest";
import { createFakeJolt } from "jolt-sdk/testing";

import { Chirp } from "./chirp";
import { Timeline } from "./timeline";

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
});
