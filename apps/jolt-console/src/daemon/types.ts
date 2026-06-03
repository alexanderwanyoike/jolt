export type DaemonStatus = {
  peer_id?: string;
  identity_address?: string;
  uptime_secs?: number;
  connected_peers?: number;
  direct_peers?: number;
  relayed_peers?: number;
  nat_type?: string;
  active_relays?: number;
  published_count?: number;
  cached_count?: number;
  bootstrap_state?: string;
  known_relay_count?: number;
  connected_bootstrap_peers?: number;
  home_relay?: {
    peer_id?: string;
    capability?: string;
    multiaddr?: string;
    api_url?: string;
  } | null;
};

export type CacheStats = {
  total_cached?: number;
  total_published?: number;
  cached_items?: number;
  published_items?: number;
  pinned_items?: number;
  pinned_size?: number;
  max_size?: number;
  available?: number;
};

export type PublishedContent = {
  content_id: string;
  size: number;
  path?: string | null;
  address?: string | null;
  pin_state?: string;
  relay?: { peer_id?: string } | null;
};

export type DaemonPayload = {
  status: DaemonStatus;
  cacheStats: CacheStats;
  published: PublishedContent[];
};

export type AppSessionStatus = "pending" | "active" | "rejected" | "revoked" | "expired";

export type AppSessionGrant = {
  request_id: string;
  session_id?: string | null;
  app_id: string;
  app_name: string;
  app_origin?: string | null;
  requested_identity?: string | null;
  identity?: string | null;
  requested_capabilities: string[];
  granted_capabilities: string[];
  status: AppSessionStatus;
  created_at: number;
  approved_at?: number | null;
  rejected_at?: number | null;
  revoked_at?: number | null;
  expires_at?: number | null;
  last_used_at?: number | null;
};

export type AppPermissionsPayload = {
  requests: AppSessionGrant[];
  sessions: AppSessionGrant[];
};
