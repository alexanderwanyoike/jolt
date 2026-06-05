import { useCallback, useEffect, useRef, useState } from "react";
import type { DaemonClient } from "./client";
import { loadDaemonPayload } from "./client";
import type { CacheEntry, CacheStats, DaemonStatus, PeerInfo, PublishedContent } from "./types";

export type DaemonSnapshot = {
  daemonUrl: string;
  connected: boolean;
  status: DaemonStatus | null;
  peers: PeerInfo[];
  cacheStats: CacheStats | null;
  cacheEntries: CacheEntry[];
  published: PublishedContent[];
  lastError: string | null;
  lastRefresh: Date | null;
  refresh(): Promise<boolean>;
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
    peers: [],
    cacheStats: null,
    cacheEntries: [],
    published: [],
    lastError: null,
    lastRefresh: null
  });
  const failureCount = useRef(0);

  const refresh = useCallback(async () => {
    try {
      const payload = await loadDaemonPayload(client);
      failureCount.current = 0;
      setState({
        daemonUrl: client.daemonUrl,
        connected: true,
        status: payload.status,
        peers: payload.peers,
        cacheStats: payload.cacheStats,
        cacheEntries: payload.cacheEntries,
        published: payload.published,
        lastError: null,
        lastRefresh: new Date()
      });
      return true;
    } catch (error) {
      failureCount.current += 1;
      setState((previous) => ({
        ...previous,
        daemonUrl: client.daemonUrl,
        connected: false,
        lastError: error instanceof Error ? error.message : String(error)
      }));
      return false;
    }
  }, [client]);

  useEffect(() => {
    let cancelled = false;
    let timeout: number | undefined;
    const nextDelay = () =>
      failureCount.current === 0
        ? refreshIntervalMs
        : Math.min(refreshIntervalMs * 2 ** failureCount.current, 30000);
    const schedule = () => {
      if (refreshIntervalMs <= 0) return;
      timeout = window.setTimeout(async () => {
        await refresh();
        if (!cancelled) {
          schedule();
        }
      }, nextDelay());
    };

    void (async () => {
      await refresh();
      if (!cancelled) {
        schedule();
      }
    })();

    return () => {
      cancelled = true;
      if (timeout !== undefined) {
        window.clearTimeout(timeout);
      }
    };
  }, [refresh, refreshIntervalMs]);

  return { ...state, refresh };
}
