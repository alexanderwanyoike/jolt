import { describe, expect, it } from "vitest";
import { JoltTransportError } from "jolt-sdk";
import { createFakeJolt, type FakeJoltOptions } from "jolt-sdk/testing";

import {
  follow,
  loadFollows,
  loadTimeline,
  postAvailableChirp,
  postChirp,
} from "./chirp";
import {
  capabilitiesFor,
  checkChirpCompatibility,
  CHIRP_HOME_RELAY_FEATURE,
  interpretChirpCompatibility,
} from "./compatibility";
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

  it("can explicitly ask the home relay to retain a published chirp", async () => {
    const { client } = createFakeJolt("alice.jolt");

    await postAvailableChirp(client, "still here");

    await expect(client.listPublished()).resolves.toMatchObject([
      { pin_state: "relay_backed" },
    ]);
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

describe("Chirp compatibility", () => {
  // @ts-expect-error A legacy fixture is always App API v1 with no advertised features.
  const impossibleLegacyFixture: FakeJoltOptions = {
    featureDiscovery: "legacy",
    features: { "availability.home-relay-pin": 1 },
  };
  void impossibleLegacyFixture;

  it("keeps the Legacy App API Baseline usable with relay controls hidden", async () => {
    const { client } = createFakeJolt("alice.jolt", {
      featureDiscovery: "legacy",
    });

    await expect(checkChirpCompatibility(client)).resolves.toEqual({
      status: "ready",
      discovery: "legacy",
      homeRelayAvailability: "hidden",
    });
  });

  it("enables the optional relay control only when a current daemon advertises it", async () => {
    const withoutFeature = createFakeJolt("alice.jolt");
    const withFeature = createFakeJolt("alice.jolt", {
      features: { [CHIRP_HOME_RELAY_FEATURE]: 1 },
    });

    await expect(checkChirpCompatibility(withoutFeature.client)).resolves.toMatchObject({
      status: "ready",
      discovery: "advertised",
      homeRelayAvailability: "hidden",
    });
    await expect(checkChirpCompatibility(withFeature.client)).resolves.toMatchObject({
      status: "ready",
      discovery: "advertised",
      homeRelayAvailability: "available",
    });

    const legacyCapabilities = capabilitiesFor(await checkChirpCompatibility(withoutFeature.client));
    const currentCapabilities = capabilitiesFor(await checkChirpCompatibility(withFeature.client));
    expect(legacyCapabilities).not.toContain("pin:own:/chirp/*");
    expect(currentCapabilities).toContain("pin:own:/chirp/*");
  });

  it("distinguishes an unreachable Jolt daemon from an incompatible one", async () => {
    const unavailable = {
      async checkCompatibility(): Promise<never> {
        throw new JoltTransportError("Jolt is not running");
      },
    };
    const incompatible = createFakeJolt("alice.jolt", { appApi: 0 });

    await expect(checkChirpCompatibility(unavailable)).resolves.toMatchObject({
      status: "unavailable",
    });
    await expect(checkChirpCompatibility(incompatible.client)).resolves.toMatchObject({
      status: "incompatible",
    });
  });

  it("reports the required features responsible for incompatibility", async () => {
    const { client } = createFakeJolt("alice.jolt", {
      features: { "data.documents": 1 },
    });
    const result = await client.checkCompatibility({
      appApi: 1,
      requiredFeatures: { "data.documents": 2, "data.tombstones": 1 },
    });

    expect(interpretChirpCompatibility(result)).toMatchObject({
      status: "incompatible",
      missingRequiredFeatures: [
        { feature: "data.documents", requiredLevel: 2, availableLevel: 1 },
        { feature: "data.tombstones", requiredLevel: 1, availableLevel: null },
      ],
    });
  });
});
