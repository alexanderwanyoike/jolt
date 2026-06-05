import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "../daemon/client";
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
        api_url: "http://127.0.0.1:9870"
      }
    },
    cacheStats: {
      total_cached: 4096,
      total_published: 2048,
      pinned_items: 1,
      available: 8192
    },
    published: [
      {
        content_id: "bafkexamplecid000000000000000001",
        path: "/demo/post",
        size: 42,
        pin_state: "pinned"
      }
    ],
    lastError: null,
    lastRefresh: new Date("2026-06-03T21:00:00Z"),
    refresh: vi.fn(async () => undefined),
    ...overrides
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
    render(<IdentityPage snapshot={snapshot()} />);

    expect(screen.getByText("Jolt address")).toBeInTheDocument();
    expect(screen.getByText("alice.jolt")).toBeInTheDocument();
    expect(screen.getByText("12D3KooAlice")).toBeInTheDocument();
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
              created_at: 1_780_000_300
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
                "export:keys"
              ],
              granted_capabilities: [],
              status: "pending",
              created_at: 1_780_000_000
            }
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
              last_used_at: 1_780_000_400
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
              last_used_at: 1_780_000_200
            }
          ];
        }
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(async () => ({ ok: true }))
    };

    render(<AppsPage client={client} />);

    const pendingRows = await screen.findAllByRole("button", { name: /request details/i });
    expect(pendingRows[0]).toHaveTextContent("Scratch");
    expect(pendingRows[1]).toHaveTextContent("Pastey");

    const sessionRows = screen.getAllByRole("button", { name: /session details/i });
    expect(sessionRows[0]).toHaveTextContent("Pastey");
    expect(sessionRows[1]).toHaveTextContent("Notes");

    expect(screen.queryByText("create or update signed paths under /pastes/*")).not.toBeInTheDocument();

    await userEvent.click(pendingRows[1]);
    expect(screen.getAllByText("alice.jolt")).not.toHaveLength(0);
    expect(screen.getByText("create or update signed paths under /pastes/*")).toBeInTheDocument();
    expect(screen.getByText("admin-only request: cannot be approved")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Approve Pastey" })).toBeDisabled();

    await userEvent.click(sessionRows[1]);
    expect(screen.getByText("Last used 2026-05-28 20:30")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Reject Pastey" }));
    expect(client.post).toHaveBeenCalledWith("/admin/v1/app-requests/req_pastey/reject");

    await userEvent.click(screen.getByRole("button", { name: "Revoke Notes" }));
    expect(client.post).toHaveBeenCalledWith("/admin/v1/app-sessions/sess_notes/revoke");
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
      "decrypt:/pastes/*"
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
              created_at: 1_780_000_500
            }
          ];
        }
        if (path === "/admin/v1/app-sessions") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn(async () => ({ ok: true }))
    };

    render(<AppsPage client={client} />);

    await userEvent.click(await screen.findByRole("button", { name: /request details/i }));
    expect(screen.getByText("publish encrypted content under /pastes/*")).toBeInTheDocument();
    expect(screen.getByText("encrypt content under /pastes/*")).toBeInTheDocument();
    expect(screen.getByText("decrypt content under /pastes/*")).toBeInTheDocument();
    expect(screen.queryByText("admin-only request: cannot be approved")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Approve Pastey" }));
    expect(client.post).toHaveBeenCalledWith("/admin/v1/app-requests/req_private_pastey/approve", {
      identity: "alice.jolt",
      capabilities: requestedCapabilities,
      expires_at: null
    });
  });

  it("renders apps empty state when there are no requests or sessions", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/admin/v1/app-requests") return [];
        if (path === "/admin/v1/app-sessions") return [];
        throw new Error(path);
      }),
      post: vi.fn()
    };

    render(<AppsPage client={client} />);

    expect(screen.getByText(/admin\/v1\/app-requests/)).toBeInTheDocument();
    expect(screen.getByText(/admin\/v1\/app-sessions/)).toBeInTheDocument();
    expect(await screen.findByText("No pending app requests.")).toBeInTheDocument();
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
            .mock.calls.filter(([calledPath]) => calledPath === "/admin/v1/app-requests").length;
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
                  created_at: 1_780_000_000
                }
              ];
        }
        if (path === "/admin/v1/app-sessions") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(<AppsPage client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
    expect(screen.getByText("No pending app requests.")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(screen.queryByRole("button", { name: /request details/i })).toHaveTextContent(
      "Pastey"
    );
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
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(<AppsPage client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
    expect(screen.getByText(/daemon offline/)).toBeInTheDocument();
    expect(client.get).toHaveBeenCalledTimes(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(client.get).toHaveBeenCalledTimes(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(client.get).toHaveBeenCalledTimes(4);
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
            .mock.calls.filter(([calledPath]) => calledPath === "/admin/v1/app-sessions").length;
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
              revoked_at: sessionCalls < 2 ? null : 1_780_000_500
            }
          ];
        }
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(<AppsPage client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
    expect(screen.getByRole("button", { name: /session details/i })).toHaveTextContent("active");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(screen.getByRole("button", { name: /session details/i })).toHaveTextContent("revoked");
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

  it("renders settings as read-only in v0", () => {
    render(<SettingsPage />);

    expect(screen.getByText(/Settings are intentionally read-only/)).toBeInTheDocument();
  });

  it("renders diagnostics error state", () => {
    render(<DiagnosticsPage snapshot={snapshot({ lastError: "daemon request failed" })} />);

    expect(screen.getByText(/daemon request failed/)).toBeInTheDocument();
    expect(screen.getByText(/127.0.0.1:9862/)).toBeInTheDocument();
  });
});
