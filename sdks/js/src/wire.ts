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
  peer_id: string;
  identity_address: string;
  uptime_secs: number;
  connected_peers: number;
  daemon_version?: string;
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

/** One device-writer append record as enumeration returns it (`/app/v1/enumerate`). */
export type AppendRecordInfo = {
  path: string;
  content_id: string;
  device_id: string;
  device_sequence: number;
  created_at: string;
  entry_hash: string;
};
