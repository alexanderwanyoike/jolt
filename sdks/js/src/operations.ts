/**
 * Typed daemon operations over a {@link JoltTransport}.
 *
 * One function per daemon endpoint the application contract declares stable.
 * These are the lowest app-facing layer: wire DTOs in and out, no domain
 * marshalling. Most applications should use the client from
 * {@link createJoltClient} instead and drop to operations only for endpoints
 * the client does not wrap (e.g. binary publish).
 *
 * @module
 */

import type { CallOptions, JoltTransport } from "./transport.js";
import type {
  AppendRecordInfo,
  AppApiFeatureManifestResponse,
  AppSessionRequestResponse,
  AppSessionStatusResponse,
  CurrentAppSession,
  DecryptedEncryptedObject,
  DecryptedIngress,
  EncryptedPublishResponse,
  FetchResult,
  HomeRelayPinResponse,
  IngressRecord,
  NodeStatus,
  OpenedEncryptedObject,
  PublishedContent,
  PublishResponse,
  ResolveResponse,
} from "./wire.js";

/** What an app declares when opening a Jolt session. */
export type SessionRequest = {
  appId: string;
  appName: string;
  appOrigin: string;
  /** The identity whose authority the session requests, or null for local identity discovery. */
  identity: string | null;
  /** Requested capability strings, e.g. `publish:/myapp/*`. */
  capabilities: readonly string[];
};

/** Ask the daemon for a scoped session. The user approves it in Jolt Console. */
export function requestSession(
  transport: JoltTransport,
  req: SessionRequest,
  options?: CallOptions
): Promise<AppSessionRequestResponse> {
  return transport.request("app", "/sessions/request", {
    json: {
      app_id: req.appId,
      app_name: req.appName,
      app_origin: req.appOrigin,
      requested_identity: req.identity,
      requested_capabilities: req.capabilities,
    },
    ...options,
  });
}

/** Poll a session request until it carries a `session_token`. */
export function getSessionRequestStatus(
  transport: JoltTransport,
  requestId: string,
  options?: CallOptions
): Promise<AppSessionStatusResponse> {
  return transport.request("app", `/sessions/${encodeURIComponent(requestId)}`, options);
}

/** The session behind a bearer token, as the daemon sees it. */
export function getCurrentSession(
  transport: JoltTransport,
  token: string,
  options?: CallOptions
): Promise<CurrentAppSession> {
  return transport.request("app", "/session", { token, ...options });
}

/** Local daemon status: identity address, peer id, connectivity. */
export function getStatus(transport: JoltTransport, options?: CallOptions): Promise<NodeStatus> {
  return transport.request("daemon", "/status", options);
}

/** Generic App API behavior advertised by the local daemon. */
export function getAppApiFeatures(
  transport: JoltTransport,
  options?: CallOptions
): Promise<AppApiFeatureManifestResponse> {
  return transport.request("app", "/features", options);
}

/** Publish a JSON object at a signed path (last-writer-wins update log). */
export function publishJson(
  transport: JoltTransport,
  token: string,
  path: string,
  body: object,
  options?: CallOptions
): Promise<PublishResponse> {
  const jsonText = JSON.stringify(body, null, 2);
  return transport.upload("app", "/publish", {
    token,
    path,
    bytes: new TextEncoder().encode(jsonText),
    fileName: `${path.split("/").pop() || "object"}.json`,
    mimeType: "application/json",
    ...options,
  });
}

/** Publish raw bytes (images, media) at a signed path. */
export function publishBytes(
  transport: JoltTransport,
  token: string,
  path: string,
  bytes: Uint8Array,
  meta: { fileName: string; mimeType: string },
  options?: CallOptions
): Promise<PublishResponse> {
  return transport.upload("app", "/publish", {
    token,
    path,
    bytes,
    fileName: meta.fileName,
    mimeType: meta.mimeType,
    ...options,
  });
}

/**
 * Publish a coexisting device-writer append record at `path`. Append records
 * never overwrite each other, so concurrent writers are safe; read them back
 * with {@link enumerate}, never with resolve.
 */
export function appendPublishJson(
  transport: JoltTransport,
  token: string,
  path: string,
  body: object,
  options?: CallOptions
): Promise<PublishResponse> {
  const jsonText = JSON.stringify(body, null, 2);
  return transport.upload("app", "/append", {
    token,
    path,
    bytes: new TextEncoder().encode(jsonText),
    fileName: `${path.split("/").pop() || "object"}.json`,
    mimeType: "application/json",
    ...options,
  });
}

/** List an identity's append records under a path prefix. */
export function enumerate(
  transport: JoltTransport,
  token: string,
  identity: string,
  pathPrefix: string,
  options?: CallOptions
): Promise<AppendRecordInfo[]> {
  return transport.request("app", "/enumerate", {
    token,
    json: { identity, path_prefix: pathPrefix },
    ...options,
  });
}

/** Publish a JSON object encrypted to `recipients` (identity addresses). */
export function publishEncryptedJson(
  transport: JoltTransport,
  token: string,
  path: string,
  body: object,
  recipients: string[],
  options?: CallOptions
): Promise<EncryptedPublishResponse> {
  return transport.request("app", "/encrypted/publish", {
    token,
    json: {
      path,
      plaintext: Array.from(new TextEncoder().encode(JSON.stringify(body))),
      content_type: "application/json",
      recipients: recipients.map((recipient) => recipient.trim()).filter(Boolean),
    },
    ...options,
  });
}

/** Publish raw bytes encrypted to `recipients`. */
export function publishEncryptedBytes(
  transport: JoltTransport,
  token: string,
  path: string,
  plaintext: Uint8Array,
  meta: { mimeType: string; recipients: string[] },
  options?: CallOptions
): Promise<EncryptedPublishResponse> {
  return transport.request("app", "/encrypted/publish", {
    token,
    json: {
      path,
      plaintext: Array.from(plaintext),
      content_type: meta.mimeType,
      recipients: meta.recipients.map((recipient) => recipient.trim()).filter(Boolean),
    },
    ...options,
  });
}

/** Resolve a `.jolt` address (identity + path) to its current content id. */
export function resolveAddress(
  transport: JoltTransport,
  token: string,
  address: string,
  options?: CallOptions
): Promise<ResolveResponse> {
  return transport.request("app", "/resolve", { token, json: { address }, ...options });
}

/** Fetch raw bytes by content id or address. */
export function fetchTarget(
  transport: JoltTransport,
  token: string,
  target: string,
  options?: CallOptions
): Promise<FetchResult> {
  return transport.request("app", "/fetch", { token, json: { target }, ...options });
}

/** Resolve + decrypt an encrypted publication addressed to this session's identity. */
export function decryptEncryptedTarget(
  transport: JoltTransport,
  token: string,
  target: string,
  options?: CallOptions
): Promise<DecryptedEncryptedObject> {
  return transport.request("app", "/encrypted/decrypt", { token, json: { target }, ...options });
}

/** Open encrypted content, preserving ciphertext when this identity cannot decrypt it. */
export function openEncryptedTarget(
  transport: JoltTransport,
  token: string,
  target: string,
  path?: string,
  options?: CallOptions
): Promise<OpenedEncryptedObject> {
  return transport.request("app", "/encrypted/open", {
    token,
    json: { target, ...(path ? { path } : {}) },
    ...options,
  });
}

/** This node's published inventory. */
export function listPublished(
  transport: JoltTransport,
  token: string,
  options?: CallOptions
): Promise<PublishedContent[]> {
  return transport.request("app", "/published", { token, ...options });
}

/** Ask the configured home relay to retain one of this app's own publications. */
export function pinHomeRelay(
  transport: JoltTransport,
  token: string,
  contentId: string,
  path?: string,
  options?: CallOptions
): Promise<HomeRelayPinResponse> {
  return transport.request("app", "/home-relay/pins", {
    token,
    json: { content_id: contentId, ...(path ? { path } : {}) },
    ...options,
  });
}

/**
 * Deliver an already-encrypted object to a recipient's daemon
 * (`/app/v1/ingress/send`). Most apps should use the client's `sendObject`,
 * which also publishes the sender's own encrypted copy.
 */
export function sendIngress(
  transport: JoltTransport,
  token: string,
  req: { recipient: string; encryptedObject: number[]; expiresAt?: number },
  options?: CallOptions
): Promise<IngressRecord> {
  return transport.request("app", "/ingress/send", {
    token,
    json: {
      recipient: req.recipient,
      encrypted_object: req.encryptedObject,
      expires_at: req.expiresAt,
    },
    ...options,
  });
}

/** Pending ingress envelopes awaiting review. */
export function listPendingIngress(
  transport: JoltTransport,
  token: string,
  options?: CallOptions
): Promise<IngressRecord[]> {
  return transport.request("app", "/ingress/pending", { token, ...options });
}

/** Decrypt a pending ingress envelope without deciding it. */
export function openIngress(
  transport: JoltTransport,
  token: string,
  ingressId: string,
  options?: CallOptions
): Promise<DecryptedIngress> {
  return transport.request("app", `/ingress/${encodeURIComponent(ingressId)}/open`, {
    method: "POST",
    token,
    ...options,
  });
}

/** Accept a pending ingress envelope. */
export function acceptIngress(
  transport: JoltTransport,
  token: string,
  ingressId: string,
  options?: CallOptions
): Promise<IngressRecord> {
  return transport.request("app", `/ingress/${encodeURIComponent(ingressId)}/accept`, {
    method: "POST",
    token,
    ...options,
  });
}

/** Reject a pending ingress envelope. */
export function rejectIngress(
  transport: JoltTransport,
  token: string,
  ingressId: string,
  options?: CallOptions
): Promise<IngressRecord> {
  return transport.request("app", `/ingress/${encodeURIComponent(ingressId)}/reject`, {
    method: "POST",
    token,
    ...options,
  });
}
