import { describe, expect, it } from "vitest";
import { createFakeJolt } from "jolt-sdk/testing";

import { follow, loadFollows, loadTimeline, postChirp } from "./chirp";
import { listFollowRequests, sendFollowRequest } from "./follows";

describe("chirp", () => {
  it("publishes chirps and projects a timeline, newest first", async () => {
    const { client, identity } = createFakeJolt("alice.jolt");
    await postChirp(client, "first!", () => "2026-08-05T10:00:00Z");
    await postChirp(client, "second!", () => "2026-08-05T11:00:00Z");

    const timeline = await loadTimeline(client, [identity]);
    expect(timeline.map((entry) => entry.chirp.text)).toEqual(["second!", "first!"]);
    expect(timeline.every((entry) => entry.author === "alice.jolt")).toBe(true);
  });

  it("records follow requests on the sender's side", async () => {
    const { client, sent } = createFakeJolt("alice.jolt");
    await sendFollowRequest(client, "alice.jolt", "bob.jolt", "hi!");

    expect(sent).toHaveLength(1);
    expect(sent[0]?.recipient).toBe("bob.jolt");
    expect(sent[0]?.body).toMatchObject({ kind: "chirp.follow-request", from: "alice.jolt" });
  });

  it("lists a pending follow request, accepts it, and follows back", async () => {
    const { client, identity, deliverIngress } = createFakeJolt("bob.jolt");
    deliverIngress({
      sender: "alice.jolt",
      body: { kind: "chirp.follow-request", from: "alice.jolt" },
    });

    const pending = await listFollowRequests(client);
    expect(pending.map((entry) => entry.request.from)).toEqual(["alice.jolt"]);

    const accepted = pending[0]!;
    await client.acceptIngress(accepted.ingressId);
    await follow(client, identity, accepted.request.from);

    expect(await client.listPendingIngress()).toHaveLength(0);
    expect(await loadFollows(client, identity)).toEqual(["alice.jolt"]);
  });

  it("rejects envelopes whose claimed sender does not match", async () => {
    const { client, deliverIngress } = createFakeJolt("bob.jolt");
    deliverIngress({
      sender: "mallory.jolt",
      body: { kind: "chirp.follow-request", from: "alice.jolt" },
    });

    expect(await listFollowRequests(client)).toHaveLength(0);
    expect(await client.listPendingIngress()).toHaveLength(0);
  });
});
