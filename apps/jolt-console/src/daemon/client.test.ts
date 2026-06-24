import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  addBootstrapRelay,
  clearHomeRelay,
  createLocalIdentity,
  deleteLocalIdentity,
  exportIdentity,
  importIdentity,
  loadAppPermissions,
  loadDaemonPayload,
  loadNetworkSettings,
  removeBootstrapRelay,
  selectLocalIdentity,
  setHomeRelay,
  tauriDaemonClient,
  type DaemonClient,
} from "./client";
import { tauriDaemonLifecycleClient } from "./lifecycle";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("tauriDaemonClient", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("routes daemon reads through the Tauri daemon_get command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ ok: true });

    await expect(tauriDaemonClient.get("/api/v1/status")).resolves.toEqual({
      ok: true,
    });
    expect(invoke).toHaveBeenCalledWith("daemon_get", {
      path: "/api/v1/status",
    });
  });

  it("routes daemon writes through the Tauri daemon_post command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ ok: true });

    await expect(
      tauriDaemonClient.post("/admin/v1/app-requests/req_1/approve", {
        identity: "alice.jolt",
        capabilities: ["resolve:public"],
      }),
    ).resolves.toEqual({ ok: true });
    expect(invoke).toHaveBeenCalledWith("daemon_post", {
      path: "/admin/v1/app-requests/req_1/approve",
      body: {
        identity: "alice.jolt",
        capabilities: ["resolve:public"],
      },
    });
  });

  it("routes daemon deletes through the Tauri daemon_delete command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ ok: true });

    await expect(
      tauriDaemonClient.delete!("/admin/v1/identities/work.jolt"),
    ).resolves.toEqual({
      ok: true,
    });
    expect(invoke).toHaveBeenCalledWith("daemon_delete", {
      path: "/admin/v1/identities/work.jolt",
    });
  });
});

describe("tauriDaemonLifecycleClient", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("routes lifecycle reads and actions through Tauri lifecycle commands", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ ownership: "none" })
      .mockResolvedValueOnce({ ownership: "console" })
      .mockResolvedValueOnce({ ownership: "console" })
      .mockResolvedValueOnce({ ownership: "none" });

    await expect(tauriDaemonLifecycleClient.status()).resolves.toEqual({
      ownership: "none",
    });
    await expect(tauriDaemonLifecycleClient.start()).resolves.toEqual({
      ownership: "console",
    });
    await expect(tauriDaemonLifecycleClient.restart()).resolves.toEqual({
      ownership: "console",
    });
    await expect(tauriDaemonLifecycleClient.stop()).resolves.toEqual({
      ownership: "none",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "daemon_lifecycle_status");
    expect(invoke).toHaveBeenNthCalledWith(2, "daemon_lifecycle_start");
    expect(invoke).toHaveBeenNthCalledWith(3, "daemon_lifecycle_restart");
    expect(invoke).toHaveBeenNthCalledWith(4, "daemon_lifecycle_stop");
  });
});

describe("loadDaemonPayload", () => {
  it("loads status, peer inventory, cache inventory, and published content as one snapshot", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/api/v1/status") return { peer_id: "peer" };
        if (path === "/api/v1/peers") {
          return [
            {
              peer_id: "12D3KooPeer",
              is_relayed: false,
              transport: "tcp",
              remote_addr: "/ip4/127.0.0.1/tcp/4001",
            },
          ];
        }
        if (path === "/api/v1/cache/stats") return { total_cached: 10 };
        if (path === "/api/v1/cache/entries") {
          return [
            {
              content_id: "bafkcacheentry",
              size: 10,
              cached_at: 1_780_000_000,
              last_accessed: 1_780_000_100,
              pinned: true,
            },
          ];
        }
        if (path === "/api/v1/published") {
          return [
            { content_id: "cid", size: 1, address: "alice.jolt/demo" },
            { content_id: "other", size: 1, address: "work.jolt/demo" },
          ];
        }
        if (path === "/admin/v1/identities") {
          return {
            active_identity: "alice.jolt",
            identities: [
              { address: "alice.jolt", label: "Default", active: true },
            ],
          };
        }
        throw new Error(path);
      }),
      post: vi.fn(),
    };

    await expect(loadDaemonPayload(client)).resolves.toEqual({
      status: { peer_id: "peer" },
      peers: [
        {
          peer_id: "12D3KooPeer",
          is_relayed: false,
          transport: "tcp",
          remote_addr: "/ip4/127.0.0.1/tcp/4001",
        },
      ],
      cacheStats: { total_cached: 10 },
      cacheEntries: [
        {
          content_id: "bafkcacheentry",
          size: 10,
          cached_at: 1_780_000_000,
          last_accessed: 1_780_000_100,
          pinned: true,
        },
      ],
      published: [{ content_id: "cid", size: 1, address: "alice.jolt/demo" }],
      localIdentities: {
        active_identity: "alice.jolt",
        identities: [{ address: "alice.jolt", label: "Default", active: true }],
      },
    });
  });
});

describe("local identity helpers", () => {
  it("routes local identity creation, selection, and deletion through admin endpoints", async () => {
    const identities = {
      active_identity: "work.jolt",
      identities: [
        { address: "alice.jolt", label: "Default", active: false },
        { address: "work.jolt", label: "Work", active: true },
      ],
    };
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(),
      post: vi
        .fn()
        .mockResolvedValueOnce({
          address: "work.jolt",
          label: "Work",
          active: false,
        })
        .mockResolvedValueOnce(identities),
      delete: vi
        .fn()
        .mockResolvedValueOnce({
          active_identity: "alice.jolt",
          identities: [],
        }),
    };

    await expect(createLocalIdentity(client, "Work")).resolves.toEqual({
      address: "work.jolt",
      label: "Work",
      active: false,
    });
    await expect(selectLocalIdentity(client, "work.jolt")).resolves.toEqual(
      identities,
    );
    await expect(deleteLocalIdentity(client, "work.jolt")).resolves.toEqual({
      active_identity: "alice.jolt",
      identities: [],
    });

    expect(client.post).toHaveBeenNthCalledWith(1, "/admin/v1/identities", {
      label: "Work",
    });
    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/admin/v1/identities/active",
      {
        identity: "work.jolt",
      },
    );
    expect(client.delete).toHaveBeenCalledWith(
      "/admin/v1/identities/work.jolt",
    );
  });
});

describe("identity recovery helpers", () => {
  it("routes identity export and import through admin-only recovery endpoints", async () => {
    const bundle = {
      magic: "jolt.identity.export",
      version: 1,
      identity: "alice.jolt",
      created_at: 1_780_000_000,
      kdf: { name: "argon2id", salt: "salt" },
      cipher: { name: "xchacha20poly1305", nonce: "nonce" },
      ciphertext: "ciphertext",
    };
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(),
      post: vi
        .fn()
        .mockResolvedValueOnce({
          identity: "alice.jolt",
          encryption_key_count: 1,
          bundle,
        })
        .mockResolvedValueOnce({
          identity: "alice.jolt",
          imported: true,
          restart_required: true,
          encryption_key_count: 1,
          app_sessions_imported: false,
        }),
    };

    await expect(exportIdentity(client, "", "Laptop")).resolves.toEqual({
      identity: "alice.jolt",
      encryption_key_count: 1,
      bundle,
    });
    await expect(importIdentity(client, bundle, "", true)).resolves.toEqual({
      identity: "alice.jolt",
      imported: true,
      restart_required: true,
      encryption_key_count: 1,
      app_sessions_imported: false,
    });

    expect(client.post).toHaveBeenNthCalledWith(
      1,
      "/admin/v1/identities/export",
      {
        passphrase: null,
        label: "Laptop",
      },
    );
    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/admin/v1/identities/import",
      {
        passphrase: null,
        bundle,
        allow_overwrite: true,
      },
    );
  });
});

describe("loadAppPermissions", () => {
  it("loads pending requests and sessions from the admin permission endpoints", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") return [{ request_id: "req_1" }];
        if (path === "/admin/v1/app-sessions")
          return [{ session_id: "sess_1" }];
        if (path === "/admin/v1/identities") {
          return {
            active_identity: "alice.jolt",
            identities: [
              { address: "alice.jolt", label: "Default", active: true },
            ],
          };
        }
        throw new Error(path);
      }),
      post: vi.fn(),
    };

    await expect(loadAppPermissions(client)).resolves.toEqual({
      requests: [{ request_id: "req_1" }],
      sessions: [{ session_id: "sess_1" }],
      localIdentities: {
        active_identity: "alice.jolt",
        identities: [{ address: "alice.jolt", label: "Default", active: true }],
      },
    });
  });
});

describe("network settings helpers", () => {
  it("routes network settings through admin-only daemon endpoints", async () => {
    const payload = {
      configured_bootstrap_relays: [],
      built_in_bootstrap_relays: [],
      effective_bootstrap_relays: [],
      configured_bootstrap_relay_count: 0,
      built_in_bootstrap_relay_count: 0,
      effective_bootstrap_relay_count: 0,
      use_builtin_bootstrap_relays: true,
      bootstrap_relay: false,
      home_relay: null,
    };
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/network-settings") return payload;
        throw new Error(path);
      }),
      post: vi.fn(async () => payload),
    };
    const relay = "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooRelay";

    await expect(loadNetworkSettings(client)).resolves.toEqual(payload);
    await addBootstrapRelay(client, relay);
    await removeBootstrapRelay(client, relay);
    await setHomeRelay(client, {
      multiaddr: relay,
      capability: "pinning",
      api_url: "http://127.0.0.1:9870",
    });
    await clearHomeRelay(client);

    expect(client.get).toHaveBeenCalledWith("/admin/v1/network-settings");
    expect(client.post).toHaveBeenNthCalledWith(
      1,
      "/admin/v1/bootstrap-relays",
      {
        multiaddr: relay,
      },
    );
    expect(client.post).toHaveBeenNthCalledWith(
      2,
      "/admin/v1/bootstrap-relays/remove",
      {
        multiaddr: relay,
      },
    );
    expect(client.post).toHaveBeenNthCalledWith(3, "/admin/v1/home-relay", {
      multiaddr: relay,
      capability: "pinning",
      api_url: "http://127.0.0.1:9870",
    });
    expect(client.post).toHaveBeenNthCalledWith(
      4,
      "/admin/v1/home-relay/clear",
    );
  });
});
