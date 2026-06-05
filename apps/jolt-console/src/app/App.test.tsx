import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "../daemon/client";
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

    render(<ConsoleApp client={client} refreshIntervalMs={0} />);

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
        if (path === "/api/v1/published") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(<ConsoleApp client={client} refreshIntervalMs={1000} />);

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
        if (path === "/api/v1/published") return [];
        throw new Error(`unexpected path ${path}`);
      }),
      post: vi.fn()
    };

    render(<ConsoleApp client={client} refreshIntervalMs={1000} />);

    await act(async () => {});
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
});
