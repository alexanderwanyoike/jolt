import { invoke } from "@tauri-apps/api/core";
import type { CacheStats, DaemonPayload, DaemonStatus, PublishedContent } from "./types";

export const DEFAULT_DAEMON_URL = "http://127.0.0.1:9862";

export type DaemonClient = {
  daemonUrl: string;
  get<T>(path: string): Promise<T>;
};

export const tauriDaemonClient: DaemonClient = {
  daemonUrl: DEFAULT_DAEMON_URL,
  get<T>(path: string) {
    return invoke<T>("daemon_get", { path });
  }
};

export async function loadDaemonPayload(client: DaemonClient): Promise<DaemonPayload> {
  const [status, cacheStats, published] = await Promise.all([
    client.get<DaemonStatus>("/api/v1/status"),
    client.get<CacheStats>("/api/v1/cache/stats"),
    client.get<PublishedContent[]>("/api/v1/published")
  ]);

  return { status, cacheStats, published };
}
