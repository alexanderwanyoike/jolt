import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "../daemon/client";
import type { DaemonLifecycleClient } from "../daemon/lifecycle";
import type { ConsoleUpdateClient } from "../update/client";
import { ConsoleApp } from "./App";

afterEach(() => {
  vi.useRealTimers();
  cleanup();
  window.location.hash = "";
});

describe("ConsoleApp", () => {
  it("loads daemon state and routes between console sections", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/api/v1/status") {
          return {
            identity_address: "alice.jolt",
            peer_id: "12D3KooAlice",
            connected_peers: 3,
            direct_peers: 2,
            relayed_peers: 1,
            active_relays: 1,
            published_count: 1,
            cached_count: 4,
            bootstrap_state: "connected",
            known_relay_count: 2,
            connected_bootstrap_peers: 1
          };
        }
        if (path === "/api/v1/cache/stats") {
          return {
            total_cached: 4096,
            total_published: 2048,
            pinned_items: 1,
            available: 8192
          };
        }
        const inventory = defaultInventoryEndpoint(path);
        if (inventory !== undefined) return inventory;
        if (path === "/api/v1/published") {
          return [
            {
              content_id: "bafkexamplecid000000000000000001",
              path: "/demo/post",
              size: 42,
              pin_state: "pinned"
            }
          ];
        }
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };
    const lifecycleClient = healthyLifecycleClient();

    render(<ConsoleApp client={client} lifecycleClient={lifecycleClient} refreshIntervalMs={0} />);

    expect(await screen.findAllByText("connected")).not.toHaveLength(0);
    expect(screen.getByText("3")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("link", { name: "Identity" }));
    expect(await screen.findByText("alice.jolt")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("link", { name: "Published" }));
    expect(await screen.findByText("/demo/post")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("link", { name: "Diagnostics" }));
    expect(await screen.findByText(/12D3KooAlice/)).toBeInTheDocument();
  });

  it("updates daemon status summaries without manual refresh", async () => {
    vi.useFakeTimers();
    let statusCalls = 0;
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/api/v1/status") {
          statusCalls += 1;
          return {
            identity_address: "alice.jolt",
            peer_id: "12D3KooAlice",
            connected_peers: statusCalls === 1 ? 17 : 29,
            published_count: 1,
            cached_count: 4,
            bootstrap_state: "connected",
            known_relay_count: 2
          };
        }
        if (path === "/api/v1/cache/stats") return {};
        const inventory = defaultInventoryEndpoint(path);
        if (inventory !== undefined) return inventory;
        if (path === "/api/v1/published") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(
      <ConsoleApp client={client} lifecycleClient={healthyLifecycleClient()} refreshIntervalMs={1000} />
    );

    await act(async () => {});
    expect(screen.getByText("17")).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(screen.getByText("29")).toBeInTheDocument();
  });

  it("backs off daemon status polling after an API failure", async () => {
    vi.useFakeTimers();
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/api/v1/status") {
          throw new Error("daemon offline");
        }
        if (path === "/api/v1/cache/stats") return {};
        const inventory = defaultInventoryEndpoint(path);
        if (inventory !== undefined) return inventory;
        if (path === "/api/v1/published") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(
      <ConsoleApp client={client} lifecycleClient={healthyLifecycleClient()} refreshIntervalMs={1000} />
    );

    await act(async () => {});
    expect(client.get).toHaveBeenCalledTimes(5);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(client.get).toHaveBeenCalledTimes(5);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(client.get).toHaveBeenCalledTimes(10);
  });

  it("starts the local daemon automatically when Console opens and no daemon is running", async () => {
    let started = false;
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (!started) throw new Error("daemon offline");
        if (path === "/api/v1/status") {
          return {
            identity_address: "alice.jolt",
            connected_peers: 3,
            bootstrap_state: "connected"
          };
        }
        if (path === "/api/v1/cache/stats") return {};
        const inventory = defaultInventoryEndpoint(path);
        if (inventory !== undefined) return inventory;
        if (path === "/api/v1/published") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };
    const lifecycleClient: DaemonLifecycleClient = {
      status: vi.fn(async () => ({
        daemon_url: "http://127.0.0.1:9862",
        reachability: "unavailable",
        ownership: "none",
        message: "No local daemon is responding"
      })),
      start: vi.fn(async () => {
        started = true;
        return {
          daemon_url: "http://127.0.0.1:9862",
          reachability: "healthy",
          ownership: "console",
          message: "Console owns this daemon"
        };
      }),
      stop: vi.fn(),
      restart: vi.fn()
    };

    render(<ConsoleApp client={client} lifecycleClient={lifecycleClient} refreshIntervalMs={0} />);

    expect(await screen.findAllByText("connected")).not.toHaveLength(0);
    expect(lifecycleClient.start).toHaveBeenCalledOnce();
  });

  it("checks for Console updates when the app opens", async () => {
    const updateClient: ConsoleUpdateClient = {
      check: vi.fn(async () => ({
        available: true,
        version: "0.2.0",
        currentVersion: "0.1.0"
      })),
      installAndRelaunch: vi.fn()
    };

    render(
      <ConsoleApp
        client={healthyDaemonClient()}
        lifecycleClient={healthyLifecycleClient()}
        updateClient={updateClient}
        refreshIntervalMs={0}
      />
    );

    expect(await screen.findByRole("link", { name: "Update 0.2.0" })).toBeInTheDocument();
    expect(updateClient.check).toHaveBeenCalledOnce();
  });

  it("shows console and daemon versions in the shell", async () => {
    render(
      <ConsoleApp
        client={healthyDaemonClient({ daemon_version: "8.7.6" })}
        lifecycleClient={healthyLifecycleClient()}
        consoleVersion="9.8.7"
        refreshIntervalMs={0}
      />
    );

    expect(await screen.findByText("Console v9.8.7")).toBeInTheDocument();
    expect(await screen.findByText("Daemon v8.7.6")).toBeInTheDocument();
  });
});

function healthyLifecycleClient(): DaemonLifecycleClient {
  return {
    status: vi.fn(async () => ({
      daemon_url: "http://127.0.0.1:9862",
      reachability: "healthy",
      ownership: "external",
      message: "Connected to an externally started daemon"
    })),
    start: vi.fn(),
    stop: vi.fn(),
    restart: vi.fn()
  };
}

function healthyDaemonClient(statusOverrides: Record<string, unknown> = {}): DaemonClient {
  return {
    daemonUrl: "http://127.0.0.1:9862",
    get: vi.fn(async (path: string) => {
      if (path === "/api/v1/status") {
        return {
          identity_address: "alice.jolt",
          peer_id: "12D3KooAlice",
          connected_peers: 3,
          bootstrap_state: "connected",
          ...statusOverrides
        };
      }
      if (path === "/api/v1/cache/stats") return {};
      const inventory = defaultInventoryEndpoint(path);
      if (inventory !== undefined) return inventory;
      if (path === "/api/v1/published") return [];
      throw new Error(`unexpected path ${path}`);
    }),
    post: vi.fn()
  };
}

function defaultInventoryEndpoint(path: string) {
  if (path === "/api/v1/peers") return [];
  if (path === "/api/v1/cache/entries") return [];
  return undefined;
}
