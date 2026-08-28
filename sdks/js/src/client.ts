/**
 * The high-level Jolt client: domain-shaped, tolerant, fakeable.
 *
 * {@link createJoltClient} binds a transport and a session-token source into
 * the interface applications actually program against: versioned
 * publish/read, append/enumerate, encrypted objects, and recipient-controlled
 * ingress. Reads are tolerant: a missing, unreachable, or undecodable object
 * returns `null` instead of throwing, so a bad record never poisons an app's
 * projections.
 *
 * The interfaces ({@link JoltSdk}, {@link JoltEncryptedSdk},
 * {@link JoltAvailabilitySdk}, {@link JoltIngressSdk}, {@link JoltAppendSdk}) are intentionally small so
 * tests can fake exactly the capability a feature uses; `jolt-sdk/testing`
 * ships a ready-made in-memory implementation.
 *
 * @module
 */

import * as ops from "./operations.js";
import { createCompatibilityChecker } from "./compatibility.js";
import type { JoltCompatibilitySdk } from "./compatibility.js";
import { JoltApiError } from "./errors.js";
import type { CallOptions, JoltTransport } from "./transport.js";
import type {
  AppSessionRequestResponse,
  AppSessionStatusResponse,
  CurrentAppSession,
  IngressRecord,
  LocalRecordHeadResponse,
  NodeStatus,
  PublishedContent,
  PublishResponse,
} from "./wire.js";

/** The stable identity of a publication: `(identity, path)`. */
export type Reference = {
  identity: string;
  path: string;
};

/** Result of any publish, in domain (camelCase) shape. */
export type PublishResult = {
  contentId: string;
  latestSequence: number;
  path: string;
  address: string | null;
  /** Opaque stable-record revision when the daemon bound a singleton path. */
  revision?: string;
};

/**
 * A decoder is the app's schema-level reader: it validates an already-parsed
 * JSON value into a canonical type, or returns `null` to reject it. Decoders
 * never see bytes or transport concerns.
 */
export type Decoder<T> = (value: unknown) => T | null;

/** A versioned, decoded publication: what a read hands back to the app. */
export type Versioned<T> = {
  ref: Reference;
  value: T;
  latestSequence: number;
  contentId: string;
};

/** Strict resolution metadata for one logical reference, before content fetch. */
export type ResolvedReference = {
  ref: Reference;
  latestSequence: number;
  contentId: string;
};

/** One authoritative local stable record reference that has no current value. */
export type RecordMissingResult = {
  state: "missing";
  ref: Reference;
};

/** One authoritative local stable record whose current state is a Tombstone. */
export type RecordDeletedResult = {
  state: "deleted";
  ref: Reference;
  revision: string;
};

/** One present authoritative local stable record. */
export type RecordPresentResult = {
  state: "present";
  ref: Reference;
  contentId: string;
  revision: string;
  bytes: number[];
};

/** One immutable current or common-base head in a local record conflict. */
export type RecordHeadResult = RecordDeletedResult | RecordPresentResult;

/** Every current local record head, plus an unambiguous common base when known. */
export type RecordConflictResult = {
  state: "conflicted";
  ref: Reference;
  /** Canonical deterministic winner order; the final alternative wins. */
  alternatives: RecordHeadResult[];
  base?: RecordHeadResult;
};

/** Strict authoritative state for one local stable record reference. */
export type RecordReadResult =
  | RecordMissingResult
  | RecordDeletedResult
  | RecordPresentResult
  | RecordConflictResult;

/** Opaque compare-and-set context used by advanced record mutations. */
export type RecordMutationContext = {
  readonly revision: string;
  /** Every current conflict head in daemon canonical order. Omitted for ordinary CAS. */
  readonly observedRevisions?: readonly string[];
  readonly mutationId: string;
};

function recordHeadResult(
  ref: Reference,
  head: LocalRecordHeadResponse,
): RecordHeadResult {
  if (head.state === "deleted") {
    return { state: "deleted", ref, revision: head.revision };
  }
  return {
    state: "present",
    ref,
    contentId: head.content_id,
    revision: head.revision,
    bytes: head.data,
  };
}

/** One append record, marshalled into domain shape. */
export type EnumeratedRecord = {
  identity: string;
  path: string;
  contentId: string;
  deviceId: string;
  deviceSequence: number;
  createdAt: string;
  entryHash: string;
};

/** Encrypted content plus the daemon's honest decrypt/access state. */
export type OpenEncryptedResult = {
  contentId: string;
  path: string;
  status: "decrypted" | "ciphertext";
  accessStatus: "available" | "needs_rewrap" | "not_accessible";
  bytes: number[];
  size: number;
  contentType: string | null;
  decryptError: string | null;
};

/** Confirmation that a home relay accepted an availability request. */
export type HomeRelayPinResult = {
  status: string;
  relay: string;
  owner: string;
  contentId: string;
  latestSequence: number;
  size: number;
};

/** Public publish and tolerant versioned reads. */
export interface JoltSdk {
  /** Publish a JSON object at a signed path (last-writer-wins). */
  publishJson(path: string, body: object, options?: CallOptions): Promise<PublishResult>;
  /** Resolve a reference strictly, preserving daemon errors such as Tombstones. */
  resolve(ref: Reference, options?: CallOptions): Promise<ResolvedReference>;
  /**
   * Resolve, fetch, parse, and decode a publication. Returns `null` when the
   * reference is missing/unreachable or the bytes do not decode to `T`.
   */
  read<T>(ref: Reference, decode: Decoder<T>, options?: CallOptions): Promise<Versioned<T> | null>;
  /**
   * Fetch a known content id (from an enumerated append record), then parse
   * and decode it against the supplied logical reference.
   */
  readContent<T>(
    contentId: string,
    ref: Reference,
    latestSequence: number,
    decode: Decoder<T>,
    options?: CallOptions
  ): Promise<Versioned<T> | null>;
  /** Read authoritative local record state without collapsing failures into absence. */
  readRecord(ref: Reference, options?: CallOptions): Promise<RecordReadResult>;
  /** Compare-and-set one local stable record against an observed revision. */
  updateRecord(
    ref: Reference,
    body: object,
    mutation: RecordMutationContext,
    options?: CallOptions,
  ): Promise<RecordPresentResult>;
  /** Compare-and-set one present local stable record to a Tombstone. */
  deleteRecord(
    ref: Reference,
    mutation: RecordMutationContext,
    options?: CallOptions,
  ): Promise<RecordDeletedResult>;
  /** Compare-and-set one local Tombstone to new immutable content. */
  restoreRecord(
    ref: Reference,
    body: object,
    mutation: RecordMutationContext,
    options?: CallOptions,
  ): Promise<RecordPresentResult>;
}

/** Coexisting append records and their enumeration. */
export interface JoltAppendSdk {
  /** Publish a coexisting device-writer append record at `path`. */
  publishAppend(path: string, body: object, options?: CallOptions): Promise<PublishResult>;
  /** List an identity's append records under a path prefix. */
  enumerate(
    identity: string,
    pathPrefix: string,
    options?: CallOptions
  ): Promise<EnumeratedRecord[]>;
}

/** Encrypted publish and tolerant encrypted reads. */
export interface JoltEncryptedSdk {
  /**
   * Publish a JSON body encrypted to `recipients` (identity addresses).
   * Use `[self]` for an encrypt-to-self publication; the publisher can always
   * decrypt its own publications, so {@link readEncrypted} reads them back.
   */
  publishEncryptedJson(
    path: string,
    body: object,
    recipients: string[],
    options?: CallOptions
  ): Promise<PublishResult>;
  /** Resolve + decrypt + parse + decode; `null` on any failing step. */
  readEncrypted<T>(
    ref: Reference,
    decode: Decoder<T>,
    options?: CallOptions
  ): Promise<Versioned<T> | null>;
  /** Open encrypted content without hiding a ciphertext-only result. */
  openEncrypted(
    target: string,
    path?: string,
    options?: CallOptions
  ): Promise<OpenEncryptedResult>;
  /** The local node's published inventory. */
  listPublished(options?: CallOptions): Promise<PublishedContent[]>;
}

/** Explicit application-owned requests for delegated content availability. */
export interface JoltAvailabilitySdk {
  /** Ask the configured home relay to retain one of this app's own publications. */
  pinHomeRelay(
    contentId: string,
    path?: string,
    options?: CallOptions
  ): Promise<HomeRelayPinResult>;
}

/**
 * The recipient-controlled ingress door: deliver identified objects to
 * another identity's daemon and review what arrives at yours. Transport-level
 * vocabulary only; classifying payloads is the app's job.
 */
export interface JoltIngressSdk {
  /**
   * Encrypt-publish the object at `path` (the sender's own copy) and deliver
   * it to `recipient`'s daemon. Returns the publish result so the sender can
   * version its own copy.
   */
  sendObject(
    recipient: string,
    path: string,
    body: object,
    options?: CallOptions
  ): Promise<PublishResult>;
  listPendingIngress(options?: CallOptions): Promise<IngressRecord[]>;
  /** Decrypt a pending envelope and return its parsed JSON, or `null`. */
  openIngress(ingressId: string, options?: CallOptions): Promise<unknown>;
  acceptIngress(ingressId: string, options?: CallOptions): Promise<void>;
  rejectIngress(ingressId: string, options?: CallOptions): Promise<void>;
}

/** Session bootstrap and daemon status, bound to the client's transport. */
export interface JoltSessionSdk {
  /** Ask the daemon for a scoped session; the user approves it in Console. */
  requestSession(
    req: ops.SessionRequest,
    options?: CallOptions
  ): Promise<AppSessionRequestResponse>;
  /** Poll a session request until it carries a token. */
  getSessionRequestStatus(
    requestId: string,
    options?: CallOptions
  ): Promise<AppSessionStatusResponse>;
  /** The session behind the current token, as the daemon sees it. */
  getCurrentSession(options?: CallOptions): Promise<CurrentAppSession>;
  /** Local daemon status (no session required). */
  getStatus(options?: CallOptions): Promise<NodeStatus>;
}

/** Everything a typical Jolt application needs, in one object. */
export type JoltClient = JoltSdk &
  JoltAppendSdk &
  JoltEncryptedSdk &
  JoltAvailabilitySdk &
  JoltIngressSdk &
  JoltSessionSdk &
  JoltCompatibilitySdk & {
    /** The transport backing this client, for operations the client does not wrap. */
    readonly transport: JoltTransport;
  };

/** Configuration for {@link createJoltClient}. */
export type JoltClientOptions = {
  transport: JoltTransport;
  /**
   * Where the client finds the current session token. Called per request so
   * token rotation needs no client rebuild.
   */
  getSessionToken: () => string;
};

/** A stable string key for a {@link Reference}, for maps and stores. */
export function referenceKey(ref: Reference): string {
  return `${ref.identity}\u0000${ref.path}`;
}

/** The `.jolt` address a {@link Reference} resolves through. */
export function referenceTarget(ref: Reference): string {
  return `${ref.identity}${ref.path}`;
}

/** Generate a collision-resistant id with an app-chosen prefix. */
export function makeId(prefix: string): string {
  const random =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID().replace(/-/g, "")
      : Math.random().toString(36).slice(2);
  return `${prefix}_${random.slice(0, 16)}`;
}

function toPublishResult(response: PublishResponse, path: string): PublishResult {
  return {
    contentId: response.content_id,
    latestSequence: response.latest_sequence ?? 0,
    path: response.path ?? path,
    address: response.address ?? null,
    ...(response.revision === undefined || response.revision === null
      ? {}
      : { revision: response.revision }),
  };
}

function parseJsonBytes(bytes: number[]): unknown | undefined {
  try {
    return JSON.parse(new TextDecoder().decode(new Uint8Array(bytes)));
  } catch {
    return undefined;
  }
}

/** Build a {@link JoltClient} over a transport and a session-token source. */
export function createJoltClient(options: JoltClientOptions): JoltClient {
  const { transport, getSessionToken } = options;
  const checkCompatibility = createCompatibilityChecker(transport);

  async function resolveReference(
    ref: Reference,
    call?: CallOptions,
    token = getSessionToken(),
  ): Promise<ResolvedReference> {
    const resolved = await ops.resolveAddress(
      transport,
      token,
      referenceTarget(ref),
      call,
    );
    return {
      ref,
      latestSequence: resolved.latest_sequence,
      contentId: resolved.content_id,
    };
  }

  async function resolveDecode<T>(
    ref: Reference,
    getBytes: (token: string, contentId: string, call?: CallOptions) => Promise<number[]>,
    decode: Decoder<T>,
    call?: CallOptions
  ): Promise<Versioned<T> | null> {
    const token = getSessionToken();
    let resolved;
    let bytes: number[];
    try {
      resolved = await resolveReference(ref, call, token);
      bytes = await getBytes(token, resolved.contentId, call);
    } catch {
      return null; // missing or unreachable
    }
    const parsed = parseJsonBytes(bytes);
    if (parsed === undefined) return null;
    const value = decode(parsed);
    if (value === null) return null;
    return {
      ref,
      value,
      latestSequence: resolved.latestSequence,
      contentId: resolved.contentId,
    };
  }

  return {
    transport,

    checkCompatibility,

    async publishJson(path, body, call) {
      const response = await ops.publishJson(transport, getSessionToken(), path, body, call);
      return toPublishResult(response, path);
    },

    resolve: resolveReference,

    async read(ref, decode, call) {
      return resolveDecode(
        ref,
        async (token, contentId, o) => (await ops.fetchTarget(transport, token, contentId, o)).data,
        decode,
        call
      );
    },

    async readContent(contentId, ref, latestSequence, decode, call) {
      let bytes: number[];
      try {
        bytes = (await ops.fetchTarget(transport, getSessionToken(), contentId, call)).data;
      } catch {
        return null;
      }
      const parsed = parseJsonBytes(bytes);
      if (parsed === undefined) return null;
      const value = decode(parsed);
      if (value === null) return null;
      return { ref, value, latestSequence, contentId };
    },

    async readRecord(ref, call) {
      const result = await ops.readLocalRecord(
        transport,
        getSessionToken(),
        ref.path,
        call
      );
      if (result.state === "missing") {
        return { state: "missing", ref };
      }
      if (result.state === "deleted") {
        return { state: "deleted", ref, revision: result.revision };
      }
      if (result.state === "conflicted") {
        return {
          state: "conflicted",
          ref,
          alternatives: result.alternatives.map(head => recordHeadResult(ref, head)),
          ...(result.base === undefined
            ? {}
            : { base: recordHeadResult(ref, result.base) }),
        };
      }
      return {
        state: "present",
        ref,
        contentId: result.content_id,
        revision: result.revision,
        bytes: result.data,
      };
    },

    async updateRecord(ref, body, mutation, call) {
      const response = await ops.updateLocalRecord(
        transport,
        getSessionToken(),
        ref.path,
        body,
        mutation.revision,
        mutation.mutationId,
        mutation.observedRevisions,
        call,
      );
      return {
        state: "present",
        ref,
        contentId: response.content_id,
        revision: response.revision,
        bytes: response.data,
      };
    },

    async deleteRecord(ref, mutation, call) {
      const response = await ops.deleteLocalRecord(
        transport,
        getSessionToken(),
        ref.path,
        mutation.revision,
        mutation.mutationId,
        mutation.observedRevisions,
        call,
      );
      return {
        state: "deleted",
        ref,
        revision: response.revision,
      };
    },

    async restoreRecord(ref, body, mutation, call) {
      const response = await ops.restoreLocalRecord(
        transport,
        getSessionToken(),
        ref.path,
        body,
        mutation.revision,
        mutation.mutationId,
        mutation.observedRevisions,
        call,
      );
      return {
        state: "present",
        ref,
        contentId: response.content_id,
        revision: response.revision,
        bytes: response.data,
      };
    },

    async publishAppend(path, body, call) {
      const response = await ops.appendPublishJson(transport, getSessionToken(), path, body, call);
      return toPublishResult(response, path);
    },

    async enumerate(identity, pathPrefix, call) {
      const records = await ops.enumerate(transport, getSessionToken(), identity, pathPrefix, call);
      return records.map((record) => ({
        identity,
        path: record.path,
        contentId: record.content_id,
        deviceId: record.device_id,
        deviceSequence: record.device_sequence,
        createdAt: record.created_at,
        entryHash: record.entry_hash,
      }));
    },

    async publishEncryptedJson(path, body, recipients, call) {
      const response = await ops.publishEncryptedJson(
        transport,
        getSessionToken(),
        path,
        body,
        recipients,
        call
      );
      return toPublishResult(response, path);
    },

    async readEncrypted(ref, decode, call) {
      const token = getSessionToken();
      const target = referenceTarget(ref);
      let resolved;
      let plaintext: number[];
      let contentId: string;
      try {
        resolved = await ops.resolveAddress(transport, token, target, call);
        const decrypted = await ops.decryptEncryptedTarget(transport, token, target, call);
        plaintext = decrypted.plaintext;
        contentId = decrypted.content_id || resolved.content_id;
      } catch {
        return null;
      }
      const parsed = parseJsonBytes(plaintext);
      if (parsed === undefined) return null;
      const value = decode(parsed);
      if (value === null) return null;
      return { ref, value, latestSequence: resolved.latest_sequence, contentId };
    },

    async openEncrypted(target, path, call) {
      const opened = await ops.openEncryptedTarget(
        transport,
        getSessionToken(),
        target,
        path,
        call
      );
      return {
        contentId: opened.content_id,
        path: opened.path,
        status: opened.status,
        accessStatus: opened.access_status,
        bytes: opened.status === "decrypted" ? opened.plaintext ?? [] : opened.ciphertext ?? [],
        size: opened.size,
        contentType: opened.content_type ?? null,
        decryptError: opened.decrypt_error ?? null,
      };
    },

    async listPublished(call) {
      return ops.listPublished(transport, getSessionToken(), call);
    },

    async pinHomeRelay(contentId, path, call) {
      const pinned = await ops.pinHomeRelay(
        transport,
        getSessionToken(),
        contentId,
        path,
        call
      );
      return {
        status: pinned.status,
        relay: pinned.relay,
        owner: pinned.owner,
        contentId: pinned.content_id,
        latestSequence: pinned.latest_sequence,
        size: pinned.size,
      };
    },

    async sendObject(recipient, path, body, call) {
      const token = getSessionToken();
      const published = await ops.publishEncryptedJson(
        transport,
        token,
        path,
        body,
        [recipient],
        call
      );
      const bytes = await ops.fetchTarget(transport, token, published.content_id, call);
      await ops.sendIngress(
        transport,
        token,
        {
          recipient,
          encryptedObject: bytes.data,
          expiresAt: Math.floor(Date.now() / 1000) + 7 * 24 * 60 * 60,
        },
        call
      );
      return toPublishResult(published, path);
    },

    async listPendingIngress(call) {
      return ops.listPendingIngress(transport, getSessionToken(), call);
    },

    async openIngress(ingressId, call) {
      const opened = await ops.openIngress(transport, getSessionToken(), ingressId, call);
      const parsed = parseJsonBytes(opened.plaintext);
      return parsed === undefined ? null : parsed;
    },

    async acceptIngress(ingressId, call) {
      await ops.acceptIngress(transport, getSessionToken(), ingressId, call);
    },

    async rejectIngress(ingressId, call) {
      await ops.rejectIngress(transport, getSessionToken(), ingressId, call);
    },

    async requestSession(req, call) {
      return ops.requestSession(transport, req, call);
    },

    async getSessionRequestStatus(requestId, call) {
      return ops.getSessionRequestStatus(transport, requestId, call);
    },

    async getCurrentSession(call) {
      return ops.getCurrentSession(transport, getSessionToken(), call);
    },

    async getStatus(call) {
      return ops.getStatus(transport, call);
    },
  };
}
