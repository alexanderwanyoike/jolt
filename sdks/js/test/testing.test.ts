import { describe, expect, it } from "vitest";

import { createFakeJolt } from "../src/testing.js";

describe("createFakeJolt", () => {
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
