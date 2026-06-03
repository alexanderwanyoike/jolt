import { useCallback, useEffect, useState } from "react";
import type { DaemonClient } from "./client";
import { loadDaemonPayload } from "./client";
import type { CacheStats, DaemonStatus, PublishedContent } from "./types";

export type DaemonSnapshot = {
  daemonUrl: string;
  connected: boolean;
  status: DaemonStatus | null;
  cacheStats: CacheStats | null;
  published: PublishedContent[];
  lastError: string | null;
  lastRefresh: Date | null;
  refresh(): Promise<void>;
};

type SnapshotState = Omit<DaemonSnapshot, "refresh">;

export function useDaemonSnapshot(
  client: DaemonClient,
  refreshIntervalMs: number
): DaemonSnapshot {
  const [state, setState] = useState<SnapshotState>({
    daemonUrl: client.daemonUrl,
    connected: false,
    status: null,
    cacheStats: null,
    published: [],
    lastError: null,
    lastRefresh: null
  });

  const refresh = useCallback(async () => {
    try {
      const payload = await loadDaemonPayload(client);
      setState({
        daemonUrl: client.daemonUrl,
        connected: true,
        status: payload.status,
        cacheStats: payload.cacheStats,
        published: payload.published,
        lastError: null,
        lastRefresh: new Date()
      });
    } catch (error) {
      setState((previous) => ({
        ...previous,
        daemonUrl: client.daemonUrl,
        connected: false,
        lastError: error instanceof Error ? error.message : String(error)
      }));
    }
  }, [client]);

  useEffect(() => {
    void refresh();
    if (refreshIntervalMs <= 0) return undefined;

    const interval = window.setInterval(() => void refresh(), refreshIntervalMs);
    return () => window.clearInterval(interval);
  }, [refresh, refreshIntervalMs]);

  return { ...state, refresh };
}
