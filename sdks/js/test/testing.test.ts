import { describe, expect, it } from "vitest";

import { createFakeJolt } from "../src/testing.js";

describe("createFakeJolt", () => {
  it("returns the complete local status shape used by applications", async () => {
    const { client } = createFakeJolt("alice.jolt");

    await expect(client.getStatus()).resolves.toMatchObject({
      identity_address: "alice.jolt",
      direct_peers: 0,
      relayed_peers: 0,
      active_relays: 0,
      published_count: 0,
      cached_count: 0,
      bootstrap_state: "idle",
      known_relay_count: 0,
      connected_bootstrap_peers: 0,
      home_relay: null,
    });
  });

  it("matches daemon compatibility evaluation for application tests", async () => {
    const { client } = createFakeJolt("alice.jolt", {
      appApi: 1,
      features: { "data.documents": 1 },
    });

    const result = await client.checkCompatibility({
      appApi: 1,
      requiredFeatures: { "data.documents": 1 },
      optionalFeatures: { "data.subscriptions": 1 },
    });

    expect(result.status).toBe("compatible");
    expect(result.manifest).toEqual({
      appApi: 1,
      features: { "data.documents": 1 },
      discovery: "advertised",
    });
    expect(result.optionalFeatures["data.subscriptions"]?.supported).toBe(false);
  });

  it("can model the Legacy App API Baseline in application fixtures", async () => {
    const { client } = createFakeJolt("alice.jolt", {
      featureDiscovery: "legacy",
    });

    const result = await client.checkCompatibility({ appApi: 1 });

    expect(result.status).toBe("compatible");
    expect(result.manifest).toEqual({
      appApi: 1,
      features: {},
      discovery: "legacy",
    });
  });

  it("round-trips public publish and read with sequences", async () => {
    const { client } = createFakeJolt("alice.jolt");
    await client.publishJson("/app/profile", { name: "Alice" });
    await client.publishJson("/app/profile", { name: "Alice II" });

    const got = await client.read(
      { identity: "alice.jolt", path: "/app/profile" },
      (v) => v as { name: string }
    );

    expect(got?.value.name).toBe("Alice II");
    expect(got?.latestSequence).toBe(1);
  });

  it("separates public and encrypted reads", async () => {
    const { client, encryptedRecipients } = createFakeJolt("alice.jolt");
    await client.publishEncryptedJson("/app/secret", { s: 1 }, ["alice.jolt"]);

    const asPublic = await client.read(
      { identity: "alice.jolt", path: "/app/secret" },
      (v) => v as object
    );
    const asEncrypted = await client.readEncrypted(
      { identity: "alice.jolt", path: "/app/secret" },
      (v) => v as { s: number }
    );

    expect(asPublic).toBeNull();
    expect(asEncrypted?.value.s).toBe(1);
    expect(encryptedRecipients.get("/app/secret")).toEqual(["alice.jolt"]);
  });

  it("matches encrypted open behavior for app tests", async () => {
    const { client } = createFakeJolt("alice.jolt");
    await client.publishEncryptedJson("/app/secret", { s: 1 }, ["alice.jolt"]);

    const opened = await client.openEncrypted("alice.jolt/app/secret");

    expect(opened).toMatchObject({
      path: "/app/secret",
      status: "decrypted",
      accessStatus: "available",
      contentType: "application/json",
      decryptError: null,
    });
    expect(JSON.parse(new TextDecoder().decode(new Uint8Array(opened.bytes)))).toEqual({ s: 1 });
  });

  it("matches home-relay pin state transitions for app tests", async () => {
    const { client } = createFakeJolt("alice.jolt");
    const published = await client.publishJson("/app/public", { hello: "relay" });

    await expect(client.listPublished()).resolves.toMatchObject([
      { content_id: published.contentId, pin_state: "local_only" },
    ]);
    await expect(
      client.pinHomeRelay(published.contentId, "/app/public")
    ).resolves.toMatchObject({
      status: "pinned",
      contentId: published.contentId,
      latestSequence: 0,
    });
    await expect(client.listPublished()).resolves.toMatchObject([
      { content_id: published.contentId, pin_state: "relay_backed" },
    ]);
  });

  it("records sends and delivers injected ingress through the review flow", async () => {
    const fake = createFakeJolt("alice.jolt");
    await fake.client.sendObject("bob.jolt", "/app/outgoing/1", { hello: "bob" });
    expect(fake.sent).toEqual([{ recipient: "bob.jolt", path: "/app/outgoing/1", body: { hello: "bob" } }]);

    const record = fake.deliverIngress({ sender: "bob.jolt", body: { hello: "alice" } });
    const pending = await fake.client.listPendingIngress();
    expect(pending.map((r) => r.ingress_id)).toEqual([record.ingress_id]);
    await expect(fake.client.openIngress(record.ingress_id)).resolves.toEqual({ hello: "alice" });

    await fake.client.acceptIngress(record.ingress_id);
    await expect(fake.client.listPendingIngress()).resolves.toEqual([]);
    await expect(fake.client.acceptIngress(record.ingress_id)).rejects.toThrow(/not pending/);
  });

  it("enumerates appends under a prefix", async () => {
    const { client } = createFakeJolt("alice.jolt");
    await client.publishAppend("/app/posts/p1", { n: 1 });
    await client.publishAppend("/app/posts/p2", { n: 2 });
    await client.publishAppend("/app/other/x", { n: 3 });

    const records = await client.enumerate("alice.jolt", "/app/posts/");
    expect(records.map((r) => r.path).sort()).toEqual(["/app/posts/p1", "/app/posts/p2"]);

    const read = await client.readContent(
      records[0]!.contentId,
      { identity: "alice.jolt", path: records[0]!.path },
      0,
      (v) => v as { n: number }
    );
    expect(read?.value.n).toBeGreaterThan(0);
  });
});
