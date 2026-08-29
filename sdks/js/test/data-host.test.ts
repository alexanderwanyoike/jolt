import { describe, expect, it, vi } from "vitest";

import {
  connectDataApp,
  type DataAppHostClient,
  type DataAppSessionStorage,
} from "../src/data-host.js";
import {
  AppIncompatibleError,
  AppSessionRejectedError,
} from "jolt-sdk/data";
import { Read, ResourceKind } from "jolt-sdk/data";

function memoryStorage(): DataAppSessionStorage & { values: Map<string, string> } {
  const values = new Map<string, string>();
  return {
    values,
    getItem: key => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: key => { values.delete(key); },
  };
}

const app = {
  id: "chirp.example",
  name: "Chirp",
  accessPlan: {
    requirements: [{
      resource: "posts",
      kind: ResourceKind.Collection,
      access: {
        read: Read.AnyIdentity,
        create: true as const,
        update: true as const,
        delete: true as const,
        restore: true as const,
      },
    }],
    grants: [{
      resource: "posts",
      path: "/chirp/posts/*",
      access: {
        read: Read.AnyIdentity,
        create: true as const,
        update: true as const,
        delete: true as const,
        restore: true as const,
      },
    }],
    subscriptions: [{ resource: "posts", path: "/chirp/posts/*" }],
  },
};

describe("Data SDK host bootstrap", () => {
  it("derives compatibility and authority, waits for approval, and retains the session", async () => {
    const storage = memoryStorage();
    let getToken = () => "";
    const checkCompatibility = vi.fn(async () => ({
      status: "compatible" as const,
      manifest: {
        appApi: 1,
        features: { "data.records": 5, "data.subscriptions": 1 },
        discovery: "advertised" as const,
      },
      appApi: { requiredLevel: 1, availableLevel: 1, supported: true },
      requiredFeatures: {
        "data.records": { requiredLevel: 5, availableLevel: 5, supported: true },
        "data.subscriptions": { requiredLevel: 1, availableLevel: 1, supported: true },
      },
      optionalFeatures: {},
    }));
    const requestSession = vi.fn(async () => ({ request_id: "request-1", status: "pending" as const }));
    const getSessionRequestStatus = vi.fn()
      .mockResolvedValueOnce({ request_id: "request-1", status: "pending", capabilities: [] })
      .mockResolvedValueOnce({
        request_id: "request-1",
        status: "active",
        session_token: "session-1",
        identity: "alice.jolt",
        capabilities: [
          "resolve:public",
          "fetch:public",
          "publish:/chirp/posts/*",
          "delete:/chirp/posts/*",
          "subscribe:any:/chirp/posts/*",
        ],
      });
    const client = {
      checkCompatibility,
      getStatus: vi.fn(async () => ({ identity_address: "alice.jolt" })),
      getCurrentSession: vi.fn(),
      requestSession,
      getSessionRequestStatus,
    } as unknown as DataAppHostClient;

    const connected = await connectDataApp(app, {
      createClient: tokenSource => {
        getToken = tokenSource;
        return client;
      },
      storage,
      appOrigin: "tauri://chirp.example",
      sleep: async () => undefined,
    });

    expect(checkCompatibility).toHaveBeenCalledWith({
      appApi: 1,
      requiredFeatures: { "data.records": 5, "data.subscriptions": 1 },
    });
    expect(requestSession).toHaveBeenCalledWith({
      appId: "chirp.example",
      appName: "Chirp",
      appOrigin: "tauri://chirp.example",
      identity: "alice.jolt",
      capabilities: [
        "resolve:public",
        "fetch:public",
        "publish:/chirp/posts/*",
        "delete:/chirp/posts/*",
        "subscribe:any:/chirp/posts/*",
      ],
    });
    expect(connected).toEqual({ identity: "alice.jolt", client });
    expect(storage.values.get("jolt.data.session:chirp.example")).toBe("session-1");
    expect(getToken()).toBe("session-1");
  });

  it("reuses a stored active session only when it still grants the derived access", async () => {
    const storage = memoryStorage();
    storage.setItem("jolt.data.session:chirp.example", "session-1");
    let getToken = () => "";
    const client = {
      checkCompatibility: vi.fn(async () => ({
        status: "compatible",
        manifest: { appApi: 1, features: { "data.records": 5 }, discovery: "advertised" },
        appApi: { requiredLevel: 1, availableLevel: 1, supported: true },
        requiredFeatures: {},
        optionalFeatures: {},
      })),
      getStatus: vi.fn(async () => ({ identity_address: "alice.jolt" })),
      getCurrentSession: vi.fn(async () => ({
        request_id: "request-1",
        app_id: "chirp.example",
        app_name: "Chirp",
        identity: "alice.jolt",
        status: "active",
        granted_capabilities: [
          "resolve:public",
          "fetch:public",
          "publish:/chirp/posts/*",
          "delete:/chirp/posts/*",
          "subscribe:any:/chirp/posts/*",
        ],
      })),
      requestSession: vi.fn(),
      getSessionRequestStatus: vi.fn(),
    } as unknown as DataAppHostClient;

    const connected = await connectDataApp(app, {
      createClient: tokenSource => {
        getToken = tokenSource;
        return client;
      },
      storage,
      appOrigin: "tauri://chirp.example",
      sleep: async () => undefined,
    });

    expect(getToken()).toBe("session-1");
    expect(connected.identity).toBe("alice.jolt");
    expect(client.requestSession).not.toHaveBeenCalled();
  });

  it("replaces a stale stored token after fresh approval", async () => {
    const storage = memoryStorage();
    storage.setItem("jolt.data.session:chirp.example", "stale-session");
    let getToken = () => "";
    const client = {
      checkCompatibility: vi.fn(async () => ({
        status: "compatible",
        manifest: { appApi: 1, features: { "data.records": 5 }, discovery: "advertised" },
        appApi: { requiredLevel: 1, availableLevel: 1, supported: true },
        requiredFeatures: {},
        optionalFeatures: {},
      })),
      getStatus: vi.fn(async () => ({ identity_address: "alice.jolt" })),
      getCurrentSession: vi.fn(async () => {
        throw new Error("expired token");
      }),
      requestSession: vi.fn(async () => ({ request_id: "request-2", status: "pending" })),
      getSessionRequestStatus: vi.fn(async () => ({
        request_id: "request-2",
        status: "active",
        session_token: "fresh-session",
        identity: "alice.jolt",
        capabilities: [],
      })),
    } as unknown as DataAppHostClient;

    await connectDataApp(app, {
      createClient: tokenSource => {
        getToken = tokenSource;
        return client;
      },
      storage,
      appOrigin: "tauri://chirp.example",
      sleep: async () => undefined,
    });

    expect(client.requestSession).toHaveBeenCalledOnce();
    expect(storage.values.get("jolt.data.session:chirp.example")).toBe("fresh-session");
    expect(getToken()).toBe("fresh-session");
  });

  it("fails by exported error type when the node is incompatible or approval is rejected", async () => {
    const storage = memoryStorage();
    const incompatible = {
      checkCompatibility: vi.fn(async () => ({
        status: "incompatible",
        manifest: { appApi: 1, features: {}, discovery: "advertised" },
        appApi: { requiredLevel: 1, availableLevel: 1, supported: true },
        requiredFeatures: {
          "data.records": { requiredLevel: 5, availableLevel: null, supported: false },
        },
        optionalFeatures: {},
      })),
    } as unknown as DataAppHostClient;

    await expect(connectDataApp(app, {
      createClient: () => incompatible,
      storage,
      appOrigin: "tauri://chirp.example",
      sleep: async () => undefined,
    })).rejects.toBeInstanceOf(AppIncompatibleError);

    const rejected = {
      checkCompatibility: vi.fn(async () => ({
        status: "compatible",
        manifest: { appApi: 1, features: { "data.records": 5 }, discovery: "advertised" },
        appApi: { requiredLevel: 1, availableLevel: 1, supported: true },
        requiredFeatures: {},
        optionalFeatures: {},
      })),
      getStatus: vi.fn(async () => ({ identity_address: "alice.jolt" })),
      requestSession: vi.fn(async () => ({ request_id: "request-2", status: "pending" })),
      getSessionRequestStatus: vi.fn(async () => ({
        request_id: "request-2",
        status: "rejected",
        capabilities: [],
      })),
    } as unknown as DataAppHostClient;

    await expect(connectDataApp(app, {
      createClient: () => rejected,
      storage,
      appOrigin: "tauri://chirp.example",
      sleep: async () => undefined,
    })).rejects.toBeInstanceOf(AppSessionRejectedError);
  });
});
