import { invoke } from "@tauri-apps/api/core";
import type {
  AppPermissionsPayload,
  AppSessionGrant,
  CacheEntry,
  CacheStats,
  DaemonPayload,
  DaemonStatus,
  HomeRelayConfig,
  LocalIdentitiesPayload,
  LocalIdentity,
  NetworkSettingsPayload,
  PeerInfo,
  PublishedContent
} from "./types";

export const DEFAULT_DAEMON_URL = "http://127.0.0.1:9862";

export type DaemonClient = {
  daemonUrl: string;
  get<T>(path: string): Promise<T>;
  post<T>(path: string, body?: unknown): Promise<T>;
};

export const tauriDaemonClient: DaemonClient = {
  daemonUrl: DEFAULT_DAEMON_URL,
  get<T>(path: string) {
    return invoke<T>("daemon_get", { path });
  },
  post<T>(path: string, body?: unknown) {
    return invoke<T>("daemon_post", { path, body });
  }
};

export async function loadDaemonPayload(client: DaemonClient): Promise<DaemonPayload> {
  const [status, peers, cacheStats, cacheEntries, published, localIdentities] = await Promise.all([
    client.get<DaemonStatus>("/api/v1/status"),
    client.get<PeerInfo[]>("/api/v1/peers"),
    client.get<CacheStats>("/api/v1/cache/stats"),
    client.get<CacheEntry[]>("/api/v1/cache/entries"),
    client.get<PublishedContent[]>("/api/v1/published"),
    client.get<LocalIdentitiesPayload>("/admin/v1/identities")
  ]);

  return {
    status,
    peers,
    cacheStats,
    cacheEntries,
    published: filterPublishedForActiveIdentity(published, localIdentities.active_identity),
    localIdentities
  };
}

export async function createLocalIdentity(
  client: DaemonClient,
  label?: string
): Promise<LocalIdentity> {
  return client.post<LocalIdentity>("/admin/v1/identities", { label: label || null });
}

export async function selectLocalIdentity(
  client: DaemonClient,
  identity: string
): Promise<LocalIdentitiesPayload> {
  return client.post<LocalIdentitiesPayload>("/admin/v1/identities/active", { identity });
}

export async function loadAppPermissions(client: DaemonClient): Promise<AppPermissionsPayload> {
  const [requests, sessions, localIdentities] = await Promise.all([
    client.get<AppSessionGrant[]>("/admin/v1/app-requests"),
    client.get<AppSessionGrant[]>("/admin/v1/app-sessions"),
    client.get<LocalIdentitiesPayload>("/admin/v1/identities")
  ]);

  return { requests, sessions, localIdentities };
}

function filterPublishedForActiveIdentity(
  published: PublishedContent[],
  activeIdentity?: string | null
): PublishedContent[] {
  if (!activeIdentity) return published;
  const addressPrefix = `${activeIdentity}/`;
  return published.filter((item) => item.address?.startsWith(addressPrefix));
}

export async function loadNetworkSettings(
  client: DaemonClient
): Promise<NetworkSettingsPayload> {
  return client.get<NetworkSettingsPayload>("/admin/v1/network-settings");
}

export async function addBootstrapRelay(
  client: DaemonClient,
  multiaddr: string
): Promise<NetworkSettingsPayload> {
  return client.post<NetworkSettingsPayload>("/admin/v1/bootstrap-relays", { multiaddr });
}

export async function removeBootstrapRelay(
  client: DaemonClient,
  multiaddr: string
): Promise<NetworkSettingsPayload> {
  return client.post<NetworkSettingsPayload>("/admin/v1/bootstrap-relays/remove", { multiaddr });
}

export async function setHomeRelay(
  client: DaemonClient,
  request: Pick<HomeRelayConfig, "multiaddr" | "capability" | "api_url">
): Promise<NetworkSettingsPayload> {
  return client.post<NetworkSettingsPayload>("/admin/v1/home-relay", request);
}

export async function clearHomeRelay(client: DaemonClient): Promise<NetworkSettingsPayload> {
  return client.post<NetworkSettingsPayload>("/admin/v1/home-relay/clear");
}
