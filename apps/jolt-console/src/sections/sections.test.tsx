import "@testing-library/jest-dom/vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "../daemon/client";
import type { DaemonLifecycleClient } from "../daemon/lifecycle";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { AppsPage } from "./AppsPage";
import { CachePage } from "./CachePage";
import { DiagnosticsPage } from "./DiagnosticsPage";
import { IdentityPage } from "./IdentityPage";
import { NetworkPage } from "./NetworkPage";
import { OverviewPage } from "./OverviewPage";
import { PublishedPage } from "./PublishedPage";
import { RelaysPage } from "./RelaysPage";
import { SettingsPage } from "./SettingsPage";

afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

function snapshot(overrides: Partial<DaemonSnapshot> = {}): DaemonSnapshot {
  return {
    daemonUrl: "http://127.0.0.1:9862",
    connected: true,
    status: {
      identity_address: "alice.jolt",
      peer_id: "12D3KooAlice",
      uptime_secs: 3660,
      connected_peers: 3,
      direct_peers: 2,
      relayed_peers: 1,
      active_relays: 1,
      published_count: 1,
      cached_count: 4,
      bootstrap_state: "connected",
      known_relay_count: 2,
      connected_bootstrap_peers: 1,
      home_relay: {
        peer_id: "12D3KooRelay",
        capability: "pinning",
        multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooRelay",
        api_url: "http://127.0.0.1:9870",
      },
    },
    peers: [
      {
        peer_id: "12D3KooPeer",
        is_relayed: false,
        transport: "tcp",
        remote_addr: "/ip4/127.0.0.1/tcp/4001",
      },
    ],
    cacheStats: {
      total_cached: 4096,
      total_published: 2048,
      pinned_items: 1,
      available: 8192,
    },
    cacheEntries: [
      {
        content_id: "bafkcacheentry",
        size: 512,
        cached_at: 1_780_000_000,
        last_accessed: 1_780_000_100,
        pinned: true,
      },
    ],
    published: [
      {
        content_id: "bafkexamplecid000000000000000001",
        path: "/demo/post",
        size: 42,
        pin_state: "pinned",
      },
    ],
    localIdentities: {
      active_identity: "alice.jolt",
      identities: [
        { address: "alice.jolt", label: "Default", active: true },
        { address: "work.jolt", label: "Work", active: false },
      ],
    },
    lastError: null,
    lastRefresh: new Date("2026-06-03T21:00:00Z"),
    refresh: vi.fn(async () => undefined),
    ...overrides,
  };
}

function daemonClient(): DaemonClient {
  return {
    daemonUrl: "http://127.0.0.1:9862",
    get: vi.fn(),
    post: vi.fn(async () => ({
      active_identity: "work.jolt",
      identities: [
        { address: "alice.jolt", label: "Default", active: false },
        { address: "work.jolt", label: "Work", active: true },
      ],
    })),
    delete: vi.fn(async () => ({
      active_identity: "alice.jolt",
      identities: [{ address: "alice.jolt", label: "Default", active: true }],
    })),
  };
}

function localIdentitiesPayload() {
  return {
    active_identity: "alice.jolt",
    identities: [{ address: "alice.jolt", label: "Default", active: true }],
  };
}

describe("Console section pages", () => {
  it("renders overview daemon metrics", () => {
    render(<OverviewPage snapshot={snapshot()} />);

    expect(screen.getByText("Uptime")).toBeInTheDocument();
    expect(screen.getByText("1h 1m")).toBeInTheDocument();
    expect(screen.getByText("Bootstrap")).toBeInTheDocument();
    expect(screen.getByText("connected")).toBeInTheDocument();
  });

  it("renders the current identity page", () => {
    render(<IdentityPage client={daemonClient()} snapshot={snapshot()} />);

    expect(screen.getByText("Active local identity")).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "Name" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("columnheader", { name: "Identity" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "Default" })).toBeInTheDocument();
    expect(screen.getByText("12D3KooAlice")).toBeInTheDocument();
    expect(screen.getByRole("cell", { name: "Work" })).toBeInTheDocument();
  });

  it("selects local identities from the identity page", async () => {
    const refresh = vi.fn(async () => true);
    const client = daemonClient();

    render(<IdentityPage client={client} snapshot={snapshot({ refresh })} />);

    await userEvent.click(screen.getByRole("button", { name: "Assume Work" }));

    expect(client.post).toHaveBeenCalledWith("/admin/v1/identities/active", {
      identity: "work.jolt",
    });
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("creates named identities from the identity page", async () => {
    const refresh = vi.fn(async () => true);
    const client = daemonClient();

    render(<IdentityPage client={client} snapshot={snapshot({ refresh })} />);

    await userEvent.type(
      screen.getByLabelText("Identity name"),
      "Side project",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Create identity" }),
    );

    expect(client.post).toHaveBeenCalledWith("/admin/v1/identities", {
      label: "Side project",
    });
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("deletes generated identities from the identity page", async () => {
    const refresh = vi.fn(async () => true);
    const client = daemonClient();

    render(<IdentityPage client={client} snapshot={snapshot({ refresh })} />);

    await userEvent.click(screen.getByRole("button", { name: "Delete Work" }));

    expect(client.delete).toHaveBeenCalledWith(
      "/admin/v1/identities/work.jolt",
    );
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("exports the daemon identity after explicit private-key risk confirmation", async () => {
    const bundle = {
      magic: "jolt.identity.export",
      version: 1,
      identity: "alice.jolt",
      kdf: { name: "argon2id" },
      cipher: { name: "xchacha20poly1305" },
      salt: "salt",
      nonce: "nonce",
      ciphertext: "ciphertext",
    };
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(),
      post: vi.fn(async () => ({
        identity: "alice.jolt",
        encryption_key_count: 1,
        bundle,
      })),
    };

    render(<IdentityPage client={client} snapshot={snapshot()} />);

    expect(screen.getByRole("button", { name: "Export identity" })).toBeDisabled();
    expect(
      screen.getByText(
        "Anyone with the export file and passphrase can become this identity.",
      ),
    ).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Label"), "Laptop");
    await userEvent.type(
      screen.getAllByLabelText("Passphrase")[0],
      "correct horse battery staple",
    );
    expect(screen.getByRole("button", { name: "Export identity" })).toBeDisabled();
    await userEvent.click(
      screen.getByLabelText("I understand this exports private identity keys."),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Export identity" }),
    );

    expect(client.post).toHaveBeenCalledWith("/admin/v1/identities/export", {
      passphrase: "correct horse battery staple",
      label: "Laptop",
    });
    expect(screen.getByLabelText("Export bundle")).toHaveValue(
      JSON.stringify(bundle, null, 2),
    );
    expect(
      screen.getByText("Exported alice.jolt with 1 encryption key."),
    ).toBeInTheDocument();
  });

  it("imports an identity bundle with explicit overwrite and restart acknowledgement", async () => {
    const refresh = vi.fn(async () => true);
    const bundle = {
      magic: "jolt.identity.export",
      version: 1,
      identity: "alice.jolt",
      kdf: { name: "argon2id" },
      cipher: { name: "xchacha20poly1305" },
      salt: "salt",
      nonce: "nonce",
      ciphertext: "ciphertext",
    };
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(),
      post: vi.fn(async () => ({
        identity: "alice.jolt",
        imported: true,
        restart_required: true,
        encryption_key_count: 1,
        app_sessions_imported: false,
      })),
    };

    render(<IdentityPage client={client} snapshot={snapshot({ refresh })} />);

    fireEvent.change(screen.getByLabelText("Bundle JSON"), {
      target: { value: JSON.stringify(bundle) },
    });
    await userEvent.type(
      screen.getAllByLabelText("Passphrase")[1],
      "correct horse battery staple",
    );
    await userEvent.click(
      screen.getByLabelText("Allow replacing the existing daemon identity."),
    );
    await userEvent.click(
      screen.getByLabelText("I understand this imports private identity keys."),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Import identity" }),
    );

    expect(client.post).toHaveBeenCalledWith("/admin/v1/identities/import", {
      passphrase: "correct horse battery staple",
      bundle,
      allow_overwrite: true,
    });
    expect(
      screen.getByText(
        "Imported alice.jolt. Restart the daemon before using this identity.",
      ),
    ).toBeInTheDocument();
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("renders app permission requests and can approve, reject, and revoke grants", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") {
          return [
            {
              request_id: "req_scratch",
              app_id: "scratch.local",
              app_name: "Scratch",
              app_origin: "http://127.0.0.1:5190",
              requested_identity: "alice.jolt",
              requested_capabilities: ["resolve:public"],
              granted_capabilities: [],
              status: "pending",
              created_at: 1_780_000_300,
            },
            {
              request_id: "req_pastey",
              app_id: "pastey.local",
              app_name: "Pastey",
              app_origin: "http://127.0.0.1:5174",
              requested_identity: "alice.jolt",
              requested_capabilities: [
                "resolve:public",
                "fetch:public",
                "publish:/pastes/*",
                "pin:own:/pastes/*",
                "export:keys",
              ],
              granted_capabilities: [],
              status: "pending",
              created_at: 1_780_000_000,
            },
            {
              request_id: "req_rejected",
              app_id: "archiver.local",
              app_name: "Archiver",
              app_origin: "http://127.0.0.1:5191",
              requested_identity: "alice.jolt",
              requested_capabilities: ["resolve:public"],
              granted_capabilities: [],
              status: "rejected",
              created_at: 1_780_000_200,
              rejected_at: 1_780_000_250,
            },
          ];
        }
        if (path === "/admin/v1/app-sessions") {
          return [
            {
              request_id: "req_pastey_active",
              session_id: "sess_pastey",
              app_id: "pastey.local",
              app_name: "Pastey",
              app_origin: "http://127.0.0.1:5174",
              identity: "alice.jolt",
              requested_capabilities: ["resolve:public", "publish:/pastes/*"],
              granted_capabilities: ["resolve:public", "publish:/pastes/*"],
              status: "active",
              created_at: 1_780_000_300,
              approved_at: 1_780_000_300,
              last_used_at: 1_780_000_400,
            },
            {
              request_id: "req_notes",
              session_id: "sess_notes",
              app_id: "notes.local",
              app_name: "Notes",
              identity: "alice.jolt",
              requested_capabilities: ["resolve:public"],
              granted_capabilities: ["resolve:public"],
              status: "active",
              created_at: 1_780_000_000,
              approved_at: 1_780_000_100,
              last_used_at: 1_780_000_200,
            },
          ];
        }
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(async () => ({ ok: true })),
    };

    render(<AppsPage client={client} />);

    const pendingRows = await screen.findAllByRole("button", {
      name: /request details/i,
    });
    expect(pendingRows[0]).toHaveTextContent("Scratch");
    expect(pendingRows[1]).toHaveTextContent("Archiver");
    expect(pendingRows[1]).toHaveTextContent("rejected");
    expect(pendingRows[2]).toHaveTextContent("Pastey");

    const sessionRows = screen.getAllByRole("button", {
      name: /session details/i,
    });
    expect(sessionRows[0]).toHaveTextContent("Pastey");
    expect(sessionRows[1]).toHaveTextContent("Notes");

    expect(
      screen.queryByText("create or update signed paths under /pastes/*"),
    ).not.toBeInTheDocument();

    await userEvent.click(pendingRows[2]);
    expect(screen.getAllByText("alice.jolt")).not.toHaveLength(0);
    expect(
      screen.getByText("create or update signed paths under /pastes/*"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("admin-only request: cannot be approved"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Approve Pastey" }),
    ).toBeDisabled();

    await userEvent.click(sessionRows[1]);
    expect(screen.getByText("Last used 2026-05-28 20:30")).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Reject Pastey" }),
    );
    expect(client.post).toHaveBeenCalledWith(
      "/admin/v1/app-requests/req_pastey/reject",
    );

    await userEvent.click(screen.getByRole("button", { name: "Revoke Notes" }));
    expect(client.post).toHaveBeenCalledWith(
      "/admin/v1/app-sessions/sess_notes/revoke",
    );
  });

  it("can approve scoped encrypted Pastey capabilities", async () => {
    const requestedCapabilities = [
      "resolve:public",
      "fetch:public",
      "publish:/pastes/*",
      "publish:encrypted:/pastes/*",
      "inventory:/pastes/*",
      "pin:own:/pastes/*",
      "encrypt:/pastes/*",
      "decrypt:/pastes/*",
    ];
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") {
          return [
            {
              request_id: "req_private_pastey",
              app_id: "pastey.local",
              app_name: "Pastey",
              app_origin: "http://127.0.0.1:5174",
              requested_identity: "alice.jolt",
              requested_capabilities: requestedCapabilities,
              granted_capabilities: [],
              status: "pending",
              created_at: 1_780_000_500,
            },
          ];
        }
        if (path === "/admin/v1/app-sessions") return [];
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(async () => ({ ok: true })),
    };

    render(<AppsPage client={client} />);

    await userEvent.click(
      await screen.findByRole("button", { name: /request details/i }),
    );
    expect(
      screen.getByText("publish encrypted content under /pastes/*"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("encrypt content under /pastes/*"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("decrypt content under /pastes/*"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("admin-only request: cannot be approved"),
    ).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Approve Pastey" }),
    );
    expect(client.post).toHaveBeenCalledWith(
      "/admin/v1/app-requests/req_private_pastey/approve",
      {
        identity: "alice.jolt",
        capabilities: requestedCapabilities,
        expires_at: null,
      },
    );
  });

  it("can approve Spoke ingress review capabilities", async () => {
    const requestedCapabilities = [
      "resolve:public",
      "fetch:public",
      "publish:/spoke/*",
      "publish:encrypted:/spoke/*",
      "inventory:/spoke/*",
      "pin:own:/spoke/*",
      "encrypt:/spoke/*",
      "decrypt:/spoke/*",
      "ingress:send",
      "ingress:read",
      "ingress:decide",
    ];
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") {
          return [
            {
              request_id: "req_spoke",
              app_id: "spoke.local",
              app_name: "Spoke",
              app_origin: "http://127.0.0.1:5178",
              requested_identity: "alice.jolt",
              requested_capabilities: requestedCapabilities,
              granted_capabilities: [],
              status: "pending",
              created_at: 1_780_000_600,
            },
          ];
        }
        if (path === "/admin/v1/app-sessions") return [];
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(async () => ({ ok: true })),
    };

    render(<AppsPage client={client} />);

    await userEvent.click(
      await screen.findByRole("button", { name: /request details/i }),
    );
    expect(
      screen.getByText("send incoming app objects by identity"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("read pending incoming app objects"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("accept or reject pending incoming app objects"),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("admin-only request: cannot be approved"),
    ).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Approve Spoke" }),
    );
    expect(client.post).toHaveBeenCalledWith(
      "/admin/v1/app-requests/req_spoke/approve",
      {
        identity: "alice.jolt",
        capabilities: requestedCapabilities,
        expires_at: null,
      },
    );
  });

  it("shows the active local identity for app requests without an explicit identity", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") {
          return [
            {
              request_id: "req_selected_identity",
              app_id: "scratch.local",
              app_name: "Scratch",
              requested_identity: null,
              requested_capabilities: ["resolve:public"],
              granted_capabilities: [],
              status: "pending",
              created_at: 1_780_000_700,
            },
          ];
        }
        if (path === "/admin/v1/app-sessions") return [];
        if (path === "/admin/v1/identities") {
          return {
            active_identity: "work.jolt",
            identities: [
              { address: "alice.jolt", label: "Default", active: false },
              { address: "work.jolt", label: "Work", active: true },
            ],
          };
        }
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(async () => ({ ok: true })),
    };

    render(<AppsPage client={client} />);

    const request = await screen.findByRole("button", {
      name: /request details/i,
    });
    expect(request).toHaveTextContent("work.jolt");

    await userEvent.click(request);
    expect(screen.getAllByText("work.jolt")).not.toHaveLength(0);
  });

  it("renders apps empty state when there are no requests or sessions", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") return [];
        if (path === "/admin/v1/app-sessions") return [];
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(path);
      }),
      post: vi.fn(),
    };

    render(<AppsPage client={client} />);

    expect(screen.getByText(/admin\/v1\/app-requests/)).toBeInTheDocument();
    expect(screen.getByText(/admin\/v1\/app-sessions/)).toBeInTheDocument();
    expect(await screen.findByText("No app requests yet.")).toBeInTheDocument();
    expect(screen.getByText("No app sessions yet.")).toBeInTheDocument();
  });

  it("updates app permission requests without manual refresh", async () => {
    vi.useFakeTimers();
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") {
          const requestCalls = vi
            .mocked(client.get)
            .mock.calls.filter(
              ([calledPath]) => calledPath === "/admin/v1/app-requests",
            ).length;
          return requestCalls < 2
            ? []
            : [
                {
                  request_id: "req_pastey",
                  app_id: "pastey.local",
                  app_name: "Pastey",
                  app_origin: "http://127.0.0.1:5174",
                  requested_identity: "alice.jolt",
                  requested_capabilities: ["resolve:public"],
                  granted_capabilities: [],
                  status: "pending",
                  created_at: 1_780_000_000,
                },
              ];
        }
        if (path === "/admin/v1/app-sessions") return [];
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(),
    };

    render(<AppsPage client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
    expect(screen.getByText("No app requests yet.")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(
      screen.queryByRole("button", { name: /request details/i }),
    ).toHaveTextContent("Pastey");
  });

  it("backs off app permission polling after an API failure", async () => {
    vi.useFakeTimers();
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") {
          throw new Error("daemon offline");
        }
        if (path === "/admin/v1/app-sessions") return [];
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(),
    };

    render(<AppsPage client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
    expect(screen.getByText(/daemon offline/)).toBeInTheDocument();
    expect(client.get).toHaveBeenCalledTimes(3);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(client.get).toHaveBeenCalledTimes(3);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(client.get).toHaveBeenCalledTimes(6);
  });

  it("updates active and revoked app sessions without manual refresh", async () => {
    vi.useFakeTimers();
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") return [];
        if (path === "/admin/v1/app-sessions") {
          const sessionCalls = vi
            .mocked(client.get)
            .mock.calls.filter(
              ([calledPath]) => calledPath === "/admin/v1/app-sessions",
            ).length;
          return [
            {
              request_id: "req_pastey",
              session_id: "sess_pastey",
              app_id: "pastey.local",
              app_name: "Pastey",
              app_origin: "http://127.0.0.1:5174",
              identity: "alice.jolt",
              requested_capabilities: ["resolve:public"],
              granted_capabilities: ["resolve:public"],
              status: sessionCalls < 2 ? "active" : "revoked",
              created_at: 1_780_000_000,
              approved_at: 1_780_000_000,
              revoked_at: sessionCalls < 2 ? null : 1_780_000_500,
            },
          ];
        }
        if (path === "/admin/v1/identities") return localIdentitiesPayload();
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(),
    };

    render(<AppsPage client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
    expect(
      screen.getByRole("button", { name: /session details/i }),
    ).toHaveTextContent("active");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(
      screen.getByRole("button", { name: /session details/i }),
    ).toHaveTextContent("revoked");
  });

  it("renders network peer counts", () => {
    render(<NetworkPage snapshot={snapshot()} />);

    expect(screen.getByText("Direct peers")).toBeInTheDocument();
    expect(screen.getByText("Relayed peers")).toBeInTheDocument();
    expect(screen.getByText("Bootstrap peers")).toBeInTheDocument();
  });

  it("renders configured relay details", () => {
    render(<RelaysPage snapshot={snapshot()} />);

    expect(screen.getByText("12D3KooRelay")).toBeInTheDocument();
    expect(screen.getByText("http://127.0.0.1:9870")).toBeInTheDocument();
  });

  it("renders published content inventory", () => {
    render(<PublishedPage snapshot={snapshot()} />);

    expect(screen.getByText("/demo/post")).toBeInTheDocument();
    expect(screen.getByText("42 B - pinned")).toBeInTheDocument();
  });

  it("renders cache storage metrics", () => {
    render(<CachePage snapshot={snapshot()} />);

    expect(screen.getByText("Cached bytes")).toBeInTheDocument();
    expect(screen.getByText("4.00 KB")).toBeInTheDocument();
    expect(screen.getByText("Available")).toBeInTheDocument();
  });

  it("renders daemon lifecycle ownership and runs allowed controls", async () => {
    const states = [
      {
        daemon_url: "http://127.0.0.1:9862",
        reachability: "unavailable",
        ownership: "none",
        message: "No local daemon is responding",
      },
      {
        daemon_url: "http://127.0.0.1:9862",
        reachability: "healthy",
        ownership: "external",
        pid: 4242,
        message: "Connected to an externally started daemon",
      },
      {
        daemon_url: "http://127.0.0.1:9862",
        reachability: "healthy",
        ownership: "console",
        pid: 4343,
        message: "Console owns this daemon",
      },
    ] as const;
    let statusIndex = 0;
    const lifecycleClient: DaemonLifecycleClient = {
      status: vi.fn(
        async () => states[Math.min(statusIndex, states.length - 1)],
      ),
      start: vi.fn(async () => {
        statusIndex = 2;
        return states[2];
      }),
      stop: vi.fn(async () => {
        statusIndex = 0;
        return states[0];
      }),
      restart: vi.fn(async () => {
        statusIndex = 2;
        return states[2];
      }),
    };
    const daemonClient: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/network-settings") {
          return {
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
        }
        if (path === "/api/v1/status") return {};
        throw new Error(path);
      }),
      post: vi.fn(),
    };

    render(
      <SettingsPage
        lifecycleClient={lifecycleClient}
        daemonClient={daemonClient}
      />,
    );

    expect(
      await screen.findByText("No local daemon is responding"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Start daemon" }));
    expect(lifecycleClient.start).toHaveBeenCalledOnce();
    expect(
      await screen.findByText("Console owns this daemon"),
    ).toBeInTheDocument();

    statusIndex = 1;
    await userEvent.click(
      screen.getByRole("button", { name: "Refresh lifecycle" }),
    );
    expect(
      await screen.findByText("Connected to an externally started daemon"),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop daemon" })).toBeDisabled();
    expect(
      screen.getByText(/Console will not stop or restart it/),
    ).toBeInTheDocument();

    statusIndex = 2;
    await userEvent.click(
      screen.getByRole("button", { name: "Refresh lifecycle" }),
    );
    expect(
      await screen.findByText("Console owns this daemon"),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Restart daemon" }),
    );
    expect(lifecycleClient.restart).toHaveBeenCalledOnce();

    vi.mocked(lifecycleClient.stop).mockRejectedValueOnce(
      new Error("failed to terminate child"),
    );
    await userEvent.click(screen.getByRole("button", { name: "Stop daemon" }));
    expect(screen.getByText(/failed to terminate child/)).toBeInTheDocument();
  });

  it("renders and updates daemon network settings", async () => {
    const relay =
      "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
    const builtIn =
      "/dns4/bootstrap.jolt.test/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
    const lifecycleClient: DaemonLifecycleClient = {
      status: vi.fn(async () => ({
        daemon_url: "http://127.0.0.1:9862",
        reachability: "healthy",
        ownership: "external",
        message: "Connected to an externally started daemon",
      })),
      start: vi.fn(),
      stop: vi.fn(),
      restart: vi.fn(),
    };
    const networkPayload = {
      configured_bootstrap_relays: [relay],
      built_in_bootstrap_relays: [builtIn],
      effective_bootstrap_relays: [relay],
      configured_bootstrap_relay_count: 1,
      built_in_bootstrap_relay_count: 1,
      effective_bootstrap_relay_count: 1,
      use_builtin_bootstrap_relays: true,
      bootstrap_relay: false,
      home_relay: null,
    };
    const daemonClient: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/network-settings") return networkPayload;
        if (path === "/api/v1/status") {
          return {
            bootstrap_state: "connected",
            connected_bootstrap_peers: 1,
            known_relay_count: 2,
          };
        }
        throw new Error(path);
      }),
      post: vi.fn(async (path: string) => {
        if (path === "/admin/v1/bootstrap-relays") return networkPayload;
        if (path === "/admin/v1/bootstrap-relays/remove") {
          return { ...networkPayload, configured_bootstrap_relays: [] };
        }
        if (path === "/admin/v1/home-relay") {
          return {
            ...networkPayload,
            home_relay: {
              peer_id: "12D3KooRelay",
              multiaddr: relay,
              capability: "pinning",
              api_url: "http://127.0.0.1:9870",
            },
          };
        }
        if (path === "/admin/v1/home-relay/clear") return networkPayload;
        throw new Error(path);
      }),
    };

    render(
      <SettingsPage
        lifecycleClient={lifecycleClient}
        daemonClient={daemonClient}
      />,
    );

    expect(
      await screen.findByText("Configured bootstrap relays"),
    ).toBeInTheDocument();
    expect(screen.getByText(relay)).toBeInTheDocument();
    expect(screen.getByText(builtIn)).toBeInTheDocument();
    expect(screen.getByText("Effective at startup")).toBeInTheDocument();
    expect(screen.getByText("Bootstrap health")).toBeInTheDocument();
    expect(screen.getByText("Learned relay count")).toBeInTheDocument();

    await userEvent.clear(screen.getByLabelText("Bootstrap relay multiaddr"));
    await userEvent.type(
      screen.getByLabelText("Bootstrap relay multiaddr"),
      relay,
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Add bootstrap relay" }),
    );
    expect(daemonClient.post).toHaveBeenCalledWith(
      "/admin/v1/bootstrap-relays",
      { multiaddr: relay },
    );

    await userEvent.click(
      screen.getByRole("button", { name: "Remove bootstrap relay" }),
    );
    expect(daemonClient.post).toHaveBeenCalledWith(
      "/admin/v1/bootstrap-relays/remove",
      {
        multiaddr: relay,
      },
    );

    await userEvent.clear(screen.getByLabelText("Home relay multiaddr"));
    await userEvent.type(screen.getByLabelText("Home relay multiaddr"), relay);
    await userEvent.clear(screen.getByLabelText("Home relay API URL"));
    await userEvent.type(
      screen.getByLabelText("Home relay API URL"),
      "http://127.0.0.1:9870",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Set home relay" }),
    );
    expect(daemonClient.post).toHaveBeenCalledWith("/admin/v1/home-relay", {
      multiaddr: relay,
      capability: "pinning",
      api_url: "http://127.0.0.1:9870",
    });
    expect(await screen.findByText("12D3KooRelay")).toBeInTheDocument();

    vi.mocked(daemonClient.post).mockRejectedValueOnce(
      new Error("invalid home relay API URL"),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Set home relay" }),
    );
    expect(screen.getByText(/invalid home relay API URL/)).toBeInTheDocument();
  });

  it("installs a Console update after stopping a Console-owned daemon", async () => {
    const lifecycleClient: DaemonLifecycleClient = {
      status: vi.fn(async () => ({
        daemon_url: "http://127.0.0.1:9862",
        reachability: "healthy",
        ownership: "console",
        message: "Console owns this daemon",
      })),
      start: vi.fn(),
      stop: vi.fn(async () => ({
        daemon_url: "http://127.0.0.1:9862",
        reachability: "unavailable",
        ownership: "none",
        message: "Daemon stopped",
      })),
      restart: vi.fn(),
    };
    const daemonClient: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/network-settings") {
          return {
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
        }
        if (path === "/api/v1/status") return {};
        throw new Error(path);
      }),
      post: vi.fn(),
    };
    const updateClient = {
      check: vi.fn(async () => ({
        available: true as const,
        version: "0.2.0",
        currentVersion: "0.1.0",
        notes: "Signed update artifacts are available.",
      })),
      installAndRelaunch: vi.fn(async () => undefined),
    };

    render(
      <SettingsPage
        lifecycleClient={lifecycleClient}
        daemonClient={daemonClient}
        updateClient={updateClient}
      />,
    );

    expect(await screen.findByText("Update available")).toBeInTheDocument();
    expect(screen.getByText(/0.1.0 -> 0.2.0/)).toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: "Install and restart" }),
    );

    expect(lifecycleClient.stop).toHaveBeenCalledOnce();
    expect(updateClient.installAndRelaunch).toHaveBeenCalledOnce();
    expect(
      vi.mocked(lifecycleClient.stop).mock.invocationCallOrder[0],
    ).toBeLessThan(
      vi.mocked(updateClient.installAndRelaunch).mock.invocationCallOrder[0],
    );
  });

  it("installs a Console update without stopping an externally-owned daemon", async () => {
    const lifecycleClient: DaemonLifecycleClient = {
      status: vi.fn(async () => ({
        daemon_url: "http://127.0.0.1:9862",
        reachability: "healthy",
        ownership: "external",
        message: "Connected to an externally started daemon",
      })),
      start: vi.fn(),
      stop: vi.fn(),
      restart: vi.fn(),
    };
    const daemonClient: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/network-settings") {
          return {
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
        }
        if (path === "/api/v1/status") return {};
        throw new Error(path);
      }),
      post: vi.fn(),
    };
    const updateClient = {
      check: vi.fn(async () => ({
        available: true as const,
        version: "0.2.0",
        currentVersion: "0.1.0",
      })),
      installAndRelaunch: vi.fn(async () => undefined),
    };

    render(
      <SettingsPage
        lifecycleClient={lifecycleClient}
        daemonClient={daemonClient}
        updateClient={updateClient}
      />,
    );

    expect(await screen.findByText("Update available")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Install and restart" }),
    );

    expect(lifecycleClient.stop).not.toHaveBeenCalled();
    expect(updateClient.installAndRelaunch).toHaveBeenCalledOnce();
  });

  it("refreshes network settings after starting the daemon from Settings", async () => {
    let started = false;
    const relay =
      "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
    const lifecycleClient: DaemonLifecycleClient = {
      status: vi.fn(async () =>
        started
          ? {
              daemon_url: "http://127.0.0.1:9862",
              reachability: "healthy",
              ownership: "console",
              message: "Console owns this daemon",
            }
          : {
              daemon_url: "http://127.0.0.1:9862",
              reachability: "unavailable",
              ownership: "none",
              message: "No local daemon is responding",
            },
      ),
      start: vi.fn(async () => {
        started = true;
        return {
          daemon_url: "http://127.0.0.1:9862",
          reachability: "healthy",
          ownership: "console",
          message: "Console owns this daemon",
        };
      }),
      stop: vi.fn(),
      restart: vi.fn(),
    };
    const daemonClient: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (!started) throw new Error("daemon offline");
        if (path === "/admin/v1/network-settings") {
          return {
            configured_bootstrap_relays: [relay],
            built_in_bootstrap_relays: [],
            effective_bootstrap_relays: [relay],
            configured_bootstrap_relay_count: 1,
            built_in_bootstrap_relay_count: 0,
            effective_bootstrap_relay_count: 1,
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: false,
            home_relay: null,
          };
        }
        if (path === "/api/v1/status") {
          return {
            bootstrap_state: "connected",
            connected_bootstrap_peers: 1,
            known_relay_count: 2,
          };
        }
        throw new Error(path);
      }),
      post: vi.fn(),
    };

    render(
      <SettingsPage
        lifecycleClient={lifecycleClient}
        daemonClient={daemonClient}
      />,
    );

    expect(
      await screen.findByText("No local daemon is responding"),
    ).toBeInTheDocument();
    expect(screen.getByText(/daemon offline/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Start daemon" }));

    expect(
      await screen.findByText("Console owns this daemon"),
    ).toBeInTheDocument();
    expect(await screen.findByText(relay)).toBeInTheDocument();
    expect(screen.queryByText(/daemon offline/)).not.toBeInTheDocument();
  });

  it("renders diagnostics error state", () => {
    render(
      <DiagnosticsPage
        snapshot={snapshot({ lastError: "daemon request failed" })}
      />,
    );

    expect(screen.getByText(/daemon request failed/)).toBeInTheDocument();
    expect(screen.getByText(/127.0.0.1:9862/)).toBeInTheDocument();
  });

  it("renders diagnostics inventories from daemon APIs", () => {
    render(<DiagnosticsPage snapshot={snapshot()} />);

    expect(screen.getByText("Connected peers")).toBeInTheDocument();
    expect(screen.getByText("Cache entries")).toBeInTheDocument();
    expect(screen.getByText("12D3KooPeer")).toBeInTheDocument();
    expect(screen.getByText("/ip4/127.0.0.1/tcp/4001")).toBeInTheDocument();
    expect(screen.getByText("bafkcacheentry")).toBeInTheDocument();
    expect(screen.getByText("512 B - pinned")).toBeInTheDocument();
  });
});
