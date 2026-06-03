import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadDaemonPayload, tauriDaemonClient, type DaemonClient } from "./client";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn()
}));

describe("tauriDaemonClient", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("routes daemon reads through the Tauri daemon_get command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ ok: true });

    await expect(tauriDaemonClient.get("/api/v1/status")).resolves.toEqual({ ok: true });
    expect(invoke).toHaveBeenCalledWith("daemon_get", { path: "/api/v1/status" });
  });
});

describe("loadDaemonPayload", () => {
  it("loads status, cache stats, and published content as one snapshot", async () => {
    const client: DaemonClient = {
      daemonUrl: "http://127.0.0.1:9862",
      get: vi.fn(async (path: string) => {
        if (path === "/api/v1/status") return { peer_id: "peer" };
        if (path === "/api/v1/cache/stats") return { total_cached: 10 };
        if (path === "/api/v1/published") return [{ content_id: "cid", size: 1 }];
        throw new Error(path);
      })
    };

    await expect(loadDaemonPayload(client)).resolves.toEqual({
      status: { peer_id: "peer" },
      cacheStats: { total_cached: 10 },
      published: [{ content_id: "cid", size: 1 }]
    });
  });
});
