import { describe, expect, it } from "vitest";

import { createJoltClient, type JoltTransport } from "../src/index.js";

/** A transport that records every call and replays canned responses. */
function recordingTransport(responses: Record<string, unknown>) {
  const calls: Array<{ kind: "request" | "upload"; base: string; path: string; detail: unknown }> =
    [];
  const transport: JoltTransport = {
    async request<T>(base: string, path: string, req?: unknown): Promise<T> {
      calls.push({ kind: "request", base, path, detail: req });
      if (!(path in responses)) throw new Error(`no canned response for ${path}`);
      return responses[path] as T;
    },
    async upload<T>(base: string, path: string, req: unknown): Promise<T> {
      calls.push({ kind: "upload", base, path, detail: req });
      if (!(path in responses)) throw new Error(`no canned response for ${path}`);
      return responses[path] as T;
    },
  } as JoltTransport;
  return { transport, calls };
}

const token = () => "tok_test";

describe("createJoltClient", () => {
  it("publishJson uploads multipart JSON and marshals the result", async () => {
    const { transport, calls } = recordingTransport({
      "/publish": { content_id: "cid_1", size: 2, latest_sequence: 4, path: "/a/p" },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    const result = await jolt.publishJson("/a/p", { x: 1 });

    expect(result).toEqual({ contentId: "cid_1", latestSequence: 4, path: "/a/p", address: null });
    const call = calls[0]!;
    expect(call.kind).toBe("upload");
    const detail = call.detail as { token: string; path: string; mimeType: string };
    expect(detail.token).toBe("tok_test");
    expect(detail.path).toBe("/a/p");
    expect(detail.mimeType).toBe("application/json");
  });

  it("read resolves, fetches, and decodes; returns null when decode rejects", async () => {
    const body = { kind: "profile", name: "Alice" };
    const { transport } = recordingTransport({
      "/resolve": {
        address: "alice.jolt/a/p",
        identity: "alice",
        path: "/a/p",
        latest_sequence: 7,
        content_id: "cid_2",
        reachability_hints: [],
        source: "local",
      },
      "/fetch": { data: Array.from(new TextEncoder().encode(JSON.stringify(body))), content_id: "cid_2", size: 1 },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });
    const ref = { identity: "alice.jolt", path: "/a/p" };

    const ok = await jolt.read(ref, (v) => v as typeof body);
    expect(ok?.value).toEqual(body);
    expect(ok?.latestSequence).toBe(7);

    const rejected = await jolt.read(ref, () => null);
    expect(rejected).toBeNull();
  });

  it("read returns null instead of throwing when the daemon errors", async () => {
    const transport: JoltTransport = {
      async request(): Promise<never> {
        throw new Error("daemon down");
      },
      async upload(): Promise<never> {
        throw new Error("daemon down");
      },
    };
    const jolt = createJoltClient({ transport, getSessionToken: token });

    await expect(
      jolt.read({ identity: "a.jolt", path: "/p" }, (v) => v as object)
    ).resolves.toBeNull();
  });

  it("sendObject publishes encrypted, fetches the bytes, and ingress-sends them", async () => {
    const { transport, calls } = recordingTransport({
      "/encrypted/publish": { content_id: "cid_3", size: 9, latest_sequence: 0, recipient_count: 1 },
      "/fetch": { data: [1, 2, 3], content_id: "cid_3", size: 3 },
      "/ingress/send": {
        ingress_id: "ing_1",
        receiver_id: "r",
        sender_identity: "alice",
        recipient_identity: "bob",
        status: "pending",
        received_at: 0,
        size: 3,
      },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    const result = await jolt.sendObject("bob.jolt", "/a/outgoing/x", { hi: true });

    expect(result.contentId).toBe("cid_3");
    expect(calls.map((c) => c.path)).toEqual(["/encrypted/publish", "/fetch", "/ingress/send"]);
    const send = calls[2]!.detail as { json: { recipient: string; encrypted_object: number[] } };
    expect(send.json.recipient).toBe("bob.jolt");
    expect(send.json.encrypted_object).toEqual([1, 2, 3]);
  });

  it("enumerate marshals wire records into camelCase", async () => {
    const { transport } = recordingTransport({
      "/enumerate": [
        {
          path: "/a/posts/p1",
          content_id: "cid_4",
          device_id: "dev_a",
          device_sequence: 2,
          created_at: "2026-01-01T00:00:00Z",
          entry_hash: "h",
        },
      ],
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    const records = await jolt.enumerate("alice.jolt", "/a/posts/");

    expect(records).toEqual([
      {
        identity: "alice.jolt",
        path: "/a/posts/p1",
        contentId: "cid_4",
        deviceId: "dev_a",
        deviceSequence: 2,
        createdAt: "2026-01-01T00:00:00Z",
        entryHash: "h",
      },
    ]);
  });

  it("openIngress parses plaintext JSON and tolerates garbage", async () => {
    const payload = { schema: "x.v1" };
    const good = Array.from(new TextEncoder().encode(JSON.stringify(payload)));
    const bad = [0xff, 0x00];
    let plaintext = good;
    const transport: JoltTransport = {
      async request<T>(): Promise<T> {
        return { plaintext, size: plaintext.length, content_type: "application/json" } as T;
      },
      async upload<T>(): Promise<T> {
        throw new Error("unused");
      },
    };
    const jolt = createJoltClient({ transport, getSessionToken: token });

    await expect(jolt.openIngress("ing_1")).resolves.toEqual(payload);
    plaintext = bad;
    await expect(jolt.openIngress("ing_1")).resolves.toBeNull();
  });
});
