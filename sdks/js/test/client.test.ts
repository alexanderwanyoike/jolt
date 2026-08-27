import { describe, expect, it } from "vitest";

import {
  createJoltClient,
  JoltApiError,
  JoltTransportError,
  type JoltTransport,
} from "../src/index.js";

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
  it("evaluates required and optional App API feature levels", async () => {
    const { transport, calls } = recordingTransport({
      "/features": {
        app_api: 1,
        features: {
          "data.documents": 2,
          "data.subscriptions": 1,
        },
      },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    const result = await jolt.checkCompatibility({
      appApi: 1,
      requiredFeatures: {
        "data.documents": 1,
        "data.tombstones": 1,
      },
      optionalFeatures: {
        "data.subscriptions": 2,
      },
    });

    expect(result).toEqual({
      status: "incompatible",
      manifest: {
        appApi: 1,
        features: {
          "data.documents": 2,
          "data.subscriptions": 1,
        },
        discovery: "advertised",
      },
      appApi: { requiredLevel: 1, availableLevel: 1, supported: true },
      requiredFeatures: {
        "data.documents": { requiredLevel: 1, availableLevel: 2, supported: true },
        "data.tombstones": { requiredLevel: 1, availableLevel: null, supported: false },
      },
      optionalFeatures: {
        "data.subscriptions": { requiredLevel: 2, availableLevel: 1, supported: false },
      },
    });
    expect(calls.map(({ base, path }) => ({ base, path }))).toEqual([
      { base: "app", path: "/features" },
    ]);
  });

  it("keeps old apps compatible when daemon release metadata and features change", async () => {
    const responses = {
      "/features": {
        app_api: 1,
        daemon_version: "0.3.23",
        features: { "data.documents": 1 },
      },
    };
    const { transport } = recordingTransport(responses);
    const jolt = createJoltClient({ transport, getSessionToken: token });
    const oldAppDeclaration = { appApi: 1 };

    const beforeUpgrade = await jolt.checkCompatibility(oldAppDeclaration);
    responses["/features"] = {
      app_api: 1,
      daemon_version: "9.0.0",
      features: { "data.documents": 3 },
    };
    const afterUpgrade = await jolt.checkCompatibility(oldAppDeclaration, { refresh: true });

    expect(beforeUpgrade.status).toBe("compatible");
    expect(afterUpgrade.status).toBe("compatible");
    expect(beforeUpgrade.appApi).toEqual(afterUpgrade.appApi);
  });

  it("treats missing feature discovery as the Legacy App API v1 Baseline", async () => {
    const transport: JoltTransport = {
      async request(): Promise<never> {
        throw new JoltApiError("not found", { status: 404 });
      },
      async upload(): Promise<never> {
        throw new Error("unused");
      },
    };
    const jolt = createJoltClient({ transport, getSessionToken: token });

    const result = await jolt.checkCompatibility({
      appApi: 1,
      requiredFeatures: { "data.documents": 1 },
    });

    expect(result.status).toBe("incompatible");
    expect(result.manifest).toEqual({ appApi: 1, features: {}, discovery: "legacy" });
    expect(result.appApi).toEqual({ requiredLevel: 1, availableLevel: 1, supported: true });
    expect(result.requiredFeatures["data.documents"]).toEqual({
      requiredLevel: 1,
      availableLevel: null,
      supported: false,
    });
  });

  it("retains existing v1 operations when feature discovery selects the legacy baseline", async () => {
    const transport: JoltTransport = {
      async request(): Promise<never> {
        throw new JoltApiError("not found", { status: 404 });
      },
      async upload<T>(): Promise<T> {
        return {
          content_id: "cid_legacy",
          size: 2,
          latest_sequence: 4,
          path: "/legacy/post",
        } as T;
      },
    };
    const jolt = createJoltClient({ transport, getSessionToken: token });

    await expect(jolt.checkCompatibility({ appApi: 1 })).resolves.toMatchObject({
      status: "compatible",
      manifest: { discovery: "legacy" },
    });
    await expect(jolt.publishJson("/legacy/post", { ok: true })).resolves.toMatchObject({
      contentId: "cid_legacy",
      latestSequence: 4,
    });
  });

  it("caches one daemon manifest and refreshes it after reconnection", async () => {
    const responses = {
      "/features": { app_api: 1, features: { "data.documents": 1 } },
    };
    const { transport, calls } = recordingTransport(responses);
    const jolt = createJoltClient({ transport, getSessionToken: token });
    const declaration = { appApi: 1, requiredFeatures: { "data.documents": 1 } };

    await expect(jolt.checkCompatibility(declaration)).resolves.toMatchObject({
      status: "compatible",
    });
    responses["/features"] = { app_api: 1, features: {} };

    await expect(jolt.checkCompatibility(declaration)).resolves.toMatchObject({
      status: "compatible",
    });
    await expect(
      jolt.checkCompatibility(declaration, { refresh: true })
    ).resolves.toMatchObject({ status: "incompatible" });
    expect(calls.filter(({ path }) => path === "/features")).toHaveLength(2);
  });

  it("keeps transport unavailability distinct and retries discovery", async () => {
    const unavailable = new JoltTransportError("daemon unavailable");
    let calls = 0;
    const transport: JoltTransport = {
      async request<T>(): Promise<T> {
        calls += 1;
        if (calls === 1) throw unavailable;
        return { app_api: 1, features: {} } as T;
      },
      async upload(): Promise<never> {
        throw new Error("unused");
      },
    };
    const jolt = createJoltClient({ transport, getSessionToken: token });
    const declaration = { appApi: 1 };

    await expect(jolt.checkCompatibility(declaration)).rejects.toBe(unavailable);
    await expect(jolt.checkCompatibility(declaration)).resolves.toMatchObject({
      status: "compatible",
      manifest: { discovery: "advertised" },
    });
    expect(calls).toBe(2);
  });

  it("publishJson uploads multipart JSON and marshals the result", async () => {
    const { transport, calls } = recordingTransport({
      "/publish": {
        content_id: "cid_1",
        size: 2,
        latest_sequence: 4,
        path: "/a/p",
        revision: "revision_4",
      },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    const result = await jolt.publishJson("/a/p", { x: 1 });

    expect(result).toEqual({
      contentId: "cid_1",
      latestSequence: 4,
      path: "/a/p",
      address: null,
      revision: "revision_4",
    });
    const call = calls[0]!;
    expect(call.kind).toBe("upload");
    const detail = call.detail as { token: string; path: string; mimeType: string };
    expect(detail.token).toBe("tok_test");
    expect(detail.path).toBe("/a/p");
    expect(detail.mimeType).toBe("application/json");
  });

  it("sends opaque compare-and-set context when updating a stable record", async () => {
    const { transport, calls } = recordingTransport({
      "/records/update": {
        path: "/chirp/posts/jlt_1",
        content_id: "cid_2",
        revision: "revision_2",
        data: [123, 125],
      },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });
    const ref = { identity: "alice.jolt", path: "/chirp/posts/jlt_1" };

    await expect(jolt.updateRecord(
      ref,
      { version: 1, value: { text: "Edited" } },
      { revision: "revision_1", mutationId: "mut_1" },
    )).resolves.toEqual({
      state: "present",
      ref,
      contentId: "cid_2",
      revision: "revision_2",
      bytes: [123, 125],
    });
    expect(calls).toEqual([{
      kind: "request",
      base: "app",
      path: "/records/update",
      detail: {
        token: "tok_test",
        json: {
          path: "/chirp/posts/jlt_1",
          revision: "revision_1",
          mutation_id: "mut_1",
          data: expect.any(Array),
        },
      },
    }]);
  });

  it("can request a session before the local identity is known", async () => {
    const { transport, calls } = recordingTransport({
      "/sessions/request": { request_id: "req_1", status: "pending" },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    await jolt.requestSession({
      appId: "pastey.local",
      appName: "Pastey",
      appOrigin: "http://127.0.0.1:5174",
      identity: null,
      capabilities: ["publish:/pastes/*"],
    });

    expect(calls[0]).toMatchObject({
      path: "/sessions/request",
      detail: {
        json: {
          requested_identity: null,
          requested_capabilities: ["publish:/pastes/*"],
        },
      },
    });
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

  it("reads explicit local record state without collapsing daemon failures", async () => {
    const responses: Record<string, unknown> = {
      "/records/read": {
        state: "present",
        path: "/chirp/posts/jlt_record",
        content_id: "cid_record",
        revision: "revision_record",
        data: [1, 2, 3],
      },
    };
    const { transport, calls } = recordingTransport(responses);
    const jolt = createJoltClient({ transport, getSessionToken: token });
    const ref = { identity: "alice.jolt", path: "/chirp/posts/jlt_record" };

    await expect(jolt.readRecord(ref)).resolves.toEqual({
      state: "present",
      ref,
      contentId: "cid_record",
      revision: "revision_record",
      bytes: [1, 2, 3],
    });
    expect(calls[0]).toMatchObject({
      kind: "request",
      base: "app",
      path: "/records/read",
      detail: {
        token: "tok_test",
        json: { path: "/chirp/posts/jlt_record" },
      },
    });

    responses["/records/read"] = {
      state: "missing",
      path: "/chirp/posts/jlt_record",
    };
    await expect(jolt.readRecord(ref)).resolves.toEqual({
      state: "missing",
      ref,
    });

    responses["/records/read"] = {
      state: "deleted",
      path: "/chirp/posts/jlt_record",
      revision: "revision_tombstone",
    };
    await expect(jolt.readRecord(ref)).resolves.toEqual({
      state: "deleted",
      ref,
      revision: "revision_tombstone",
    });

    const failure = new JoltTransportError("daemon unavailable");
    const unavailableTransport: JoltTransport = {
      async request(): Promise<never> {
        throw failure;
      },
      async upload(): Promise<never> {
        throw new Error("unused");
      },
    };
    const unavailableClient = createJoltClient({
      transport: unavailableTransport,
      getSessionToken: token,
    });

    await expect(unavailableClient.readRecord(ref)).rejects.toBe(failure);
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

  it("opens encrypted content without hiding a ciphertext-only result", async () => {
    const { transport, calls } = recordingTransport({
      "/encrypted/open": {
        content_id: "cid_encrypted",
        path: "/pastes/secret",
        status: "ciphertext",
        access_status: "not_accessible",
        plaintext: null,
        ciphertext: [1, 2, 3],
        size: 3,
        content_type: null,
        decrypt_error: "no matching recipient key",
      },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    await expect(jolt.openEncrypted("alice.jolt/pastes/secret")).resolves.toEqual({
      contentId: "cid_encrypted",
      path: "/pastes/secret",
      status: "ciphertext",
      accessStatus: "not_accessible",
      bytes: [1, 2, 3],
      size: 3,
      contentType: null,
      decryptError: "no matching recipient key",
    });
    expect(calls[0]).toMatchObject({
      kind: "request",
      base: "app",
      path: "/encrypted/open",
      detail: {
        token: "tok_test",
        json: { target: "alice.jolt/pastes/secret" },
      },
    });
  });

  it("requests home-relay availability for the app's own publication", async () => {
    const { transport, calls } = recordingTransport({
      "/home-relay/pins": {
        status: "pinned",
        relay: "12D3KooWRelay",
        owner: "alice.jolt",
        content_id: "cid_public",
        latest_sequence: 4,
        size: 12,
      },
    });
    const jolt = createJoltClient({ transport, getSessionToken: token });

    await expect(jolt.pinHomeRelay("cid_public", "/pastes/hello")).resolves.toEqual({
      status: "pinned",
      relay: "12D3KooWRelay",
      owner: "alice.jolt",
      contentId: "cid_public",
      latestSequence: 4,
      size: 12,
    });
    expect(calls[0]).toMatchObject({
      kind: "request",
      base: "app",
      path: "/home-relay/pins",
      detail: {
        token: "tok_test",
        json: { content_id: "cid_public", path: "/pastes/hello" },
      },
    });
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
