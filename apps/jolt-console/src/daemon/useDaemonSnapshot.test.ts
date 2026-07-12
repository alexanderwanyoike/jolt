import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { DaemonClient } from "./client";
import type { DaemonStatus } from "./types";
import { useDaemonSnapshot } from "./useDaemonSnapshot";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

function status(identity: string): DaemonStatus {
  return {
    identity_address: identity,
    peer_id: identity,
    connected_peers: 0,
    published_count: 0,
    cached_count: 0,
    bootstrap_state: "connected",
    known_relay_count: 0
  };
}

describe("useDaemonSnapshot", () => {
  it("does not let an older poll overwrite a newer identity snapshot", async () => {
    const firstStatus = deferred<DaemonStatus>();
    const secondStatus = deferred<DaemonStatus>();
    const statusRequests = [firstStatus, secondStatus];
    let statusIndex = 0;
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/api/v1/status") {
          return statusRequests[statusIndex++].promise;
        }
        if (path === "/api/v1/cache/stats") return {};
        if (path === "/admin/v1/identities") {
          return { active_identity: "bob.jolt", identities: [] };
        }
        return [];
      }) as DaemonClient["get"],
      post: vi.fn()
    };

    const { result } = renderHook(() => useDaemonSnapshot(client, 0));
    await waitFor(() => expect(statusIndex).toBe(1));

    let newerRefresh!: Promise<boolean>;
    act(() => {
      newerRefresh = result.current.refresh();
    });
    await waitFor(() => expect(statusIndex).toBe(2));
    secondStatus.resolve(status("bob.jolt"));
    await act(async () => {
      await newerRefresh;
    });
    expect(result.current.status?.identity_address).toBe("bob.jolt");

    firstStatus.resolve(status("alice.jolt"));
    await act(async () => {});
    expect(result.current.status?.identity_address).toBe("bob.jolt");
  });
});
