/**
 * Wire DTOs: the daemon's HTTP API shapes, verbatim (snake_case).
 *
 * These types mirror the Jolt daemon's `/app/v1` and `/api/v1` responses.
 * Application code should prefer the camelCase domain types in the client
 * layer; the wire shapes are exported for apps that call operations directly.
 *
 * @module
 */

/** `/api/v1/status`: the local daemon's identity and connectivity summary. */
export type NodeStatus = {
  daemon_version: string;
  peer_id: string;
  identity_address: string;
  local_device_id: string;
  uptime_secs: number;
  connected_peers: number;
  direct_peers: number;
  relayed_peers: number;
  nat_type: string;
  active_relays: number;
  published_count: number;
  cached_count: number;
  listen_addresses: string[];
  bootstrap_relay: boolean;
  bootstrap_state: string;
  configured_bootstrap_relays: string[];
  configured_bootstrap_relay_count: number;
  effective_bootstrap_relays: string[];
  effective_bootstrap_relay_count: number;
  known_relay_count: number;
  connected_bootstrap_peers: number;
  last_bootstrap_error: string | null;
  home_relay: null | {
    peer_id: string;
    multiaddr: string;
    capability: "unknown" | "discovery_only" | "pinning";
    api_url?: string | null;
  };
  relay_record?: unknown | null;
};

/** `/app/v1/features`: generic behavior implemented by this App API. */
export type AppApiFeatureManifestResponse = {
  app_api: number;
  features: Record<string, number>;
};

/** Result of a public publish (`/app/v1/publish`). */
export type PublishResponse = {
  content_id: string;
  size: number;
  path?: string | null;
  address?: string | null;
  latest_sequence?: number | null;
  revision?: string | null;
};

/** Successful compare-and-set write of one local stable record. */
export type LocalRecordUpdateResponse = {
  path: string;
  content_id: string;
  revision: string;
  data: number[];
};

/** Successful compare-and-set restoration of one local stable record. */
export type LocalRecordRestoreResponse = LocalRecordUpdateResponse;

/** Successful compare-and-set deletion of one local stable record. */
export type LocalRecordDeleteResponse = {
  path: string;
  revision: string;
};

/** Result of an encrypted publish (`/app/v1/encrypted/publish`). */
export type EncryptedPublishResponse = PublishResponse & {
  recipient_count: number;
};

/** One locally published item (`/app/v1/published`). */
export type PublishedContent = {
  content_id: string;
  size: number;
  path?: string | null;
  address?: string | null;
  local_sequence?: number | null;
  pin_state: string;
  relay?: null | {
    peer_id: string;
    multiaddr: string;
    api_url?: string | null;
  };
  pinned_content_id?: string | null;
  pinned_sequence?: number | null;
};

/** Result of requesting availability from the configured home relay. */
export type HomeRelayPinResponse = {
  status: string;
  relay: string;
  owner: string;
  content_id: string;
  latest_sequence: number;
  size: number;
};

/** Result of resolving a `.jolt` address (`/app/v1/resolve`). */
export type ResolveResponse = {
  address: string;
  identity: string;
  path: string;
  latest_sequence: number;
  content_id: string;
  reachability_hints: unknown[];
  source: string;
};

/** Raw bytes fetched by content id (`/app/v1/fetch`). */
export type FetchResult = {
  data: number[];
  content_id: string;
  size: number;
};

/** One current or common-base head in a local record conflict. */
export type LocalRecordHeadResponse =
  | {
      state: "deleted";
      revision: string;
    }
  | {
      state: "present";
      content_id: string;
      revision: string;
      data: number[];
    };

/** Result of reading one authoritative local singleton path (`/app/v1/records/read`). */
export type LocalRecordReadResponse =
  | {
      state: "missing";
      path: string;
    }
  | ({ path: string } & LocalRecordHeadResponse)
  | {
      state: "conflicted";
      path: string;
      /** Canonical deterministic winner order; the final alternative wins. */
      alternatives: LocalRecordHeadResponse[];
      base?: LocalRecordHeadResponse;
    };

/** Lifecycle states of an app session. */
export type AppSessionStatus = "pending" | "active" | "rejected" | "revoked" | "expired";

/** Response to a session request (`/app/v1/sessions/request`). */
export type AppSessionRequestResponse = {
  request_id: string;
  status: AppSessionStatus;
};

/** Polled session request status (`/app/v1/sessions/{request_id}`). */
export type AppSessionStatusResponse = {
  request_id: string;
  session_id?: string | null;
  session_token?: string | null;
  status: AppSessionStatus;
  identity?: string | null;
  capabilities: string[];
  expires_at?: number | null;
};

/** The current session, as seen by the daemon (`/app/v1/session`). */
export type CurrentAppSession = {
  request_id: string;
  session_id?: string | null;
  app_id: string;
  app_name: string;
  identity?: string | null;
  granted_capabilities: string[];
  status: AppSessionStatus;
  expires_at?: number | null;
  last_used_at?: number | null;
};

/** One recipient-controlled ingress envelope awaiting (or past) review. */
export type IngressRecord = {
  ingress_id: string;
  receiver_id: string;
  sender_identity: string;
  recipient_identity: string;
  schema_hint?: string | null;
  status: "pending" | "accepted" | "rejected";
  received_at: number;
  expires_at?: number | null;
  size: number;
  accepted_at?: number | null;
  rejected_at?: number | null;
};

/** Decrypted ingress payload (`/app/v1/ingress/{id}/open`). */
export type DecryptedIngress = {
  plaintext: number[];
  size: number;
  content_type: string;
};

/** Decrypted encrypted publication (`/app/v1/encrypted/decrypt`). */
export type DecryptedEncryptedObject = {
  content_id: string;
  path: string;
  plaintext: number[];
  size: number;
  content_type: string;
};

/** Encrypted bytes opened with plaintext when this identity can decrypt them. */
export type OpenedEncryptedObject = {
  content_id: string;
  path: string;
  status: "decrypted" | "ciphertext";
  access_status: "available" | "needs_rewrap" | "not_accessible";
  plaintext?: number[] | null;
  ciphertext?: number[] | null;
  size: number;
  content_type?: string | null;
  decrypt_error?: string | null;
};

/** One device-writer append record as enumeration returns it (`/app/v1/enumerate`). */
export type AppendRecordInfo = {
  path: string;
  content_id: string;
  device_id: string;
  device_sequence: number;
  created_at: number;
  entry_hash: string;
};

/** One current non-deleted logical record in a Materialized View. */
export type MaterializedRecordInfo = {
  path: string;
  content_id: string;
  revision: string;
  created_at: number;
};

/** One Data Subscription's last bounded refresh state. */
export type DataSubscriptionRefreshResponse =
  | { status: "loading" }
  | { status: "updating"; last_verified_at?: number }
  | { status: "ready"; last_verified_at: number }
  | {
      status: "stale";
      last_verified_at: number;
      reason: "networkUnavailable" | "verificationFailed" | "overloaded";
    }
  | {
      status: "unavailable";
      reason: "networkUnavailable" | "verificationFailed" | "overloaded";
    };

/** Persisted Data Subscription metadata owned by the current app session. */
export type DataSubscriptionRecordResponse = {
  id: string;
  identity: string;
  prefix: string;
  lifecycle: "active" | "dormant";
  refresh: DataSubscriptionRefreshResponse;
  created_at: number;
};

/** Last verified records plus the outcome of this subscription refresh. */
export type DataSubscriptionViewResponse = {
  identity: string;
  records: MaterializedRecordInfo[];
  source: {
    subscription: string;
    state: DataSubscriptionRefreshResponse;
  };
};

/** One bounded local Materialized View event returned by a Change Stream poll. */
export type DataSubscriptionChangeResponse =
  | {
      type: "snapshot";
      cursor: string;
      identity: string;
      records: MaterializedRecordInfo[];
      state: DataSubscriptionRefreshResponse;
    }
  | {
      type: "changed";
      cursor: string;
      identity: string;
      records: MaterializedRecordInfo[];
      removed: string[];
    }
  | {
      type: "state";
      cursor: string;
      state: DataSubscriptionRefreshResponse;
    }
  | { type: "timeout"; cursor: string }
  | { type: "resync_required" }
  | { type: "cancelled" }
  | { type: "revoked" };

/** Terminal result of explicitly removing a Data Subscription. */
export type RemoveDataSubscriptionResponse = {
  status: "cancelled";
  subscription_id: string;
};
