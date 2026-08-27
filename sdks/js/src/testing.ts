/**
 * Deterministic in-memory fakes for testing Jolt applications.
 *
 * {@link createFakeJolt} returns a fully working {@link JoltClient}
 * implementation with no daemon and no network: publishes land in an
 * in-memory store keyed by path, reads resolve against it, ingress sends are
 * recorded, and incoming envelopes can be injected. Encryption is simulated
 * (recipients are recorded, plaintext is stored), which is exactly what
 * app-level tests need: they test their own schemas and flows, not HPKE.
 *
 * @module
 */

import type {
  Decoder,
  EnumeratedRecord,
  JoltClient,
  PublishResult,
  RecordDeletedResult,
  RecordPresentResult,
  Reference,
  Versioned,
} from "./client.js";
import { referenceKey } from "./client.js";
import { evaluateCompatibility } from "./compatibility.js";
import { JoltApiError } from "./errors.js";
import type { AppCompatibilityDeclaration } from "./compatibility.js";
import type { CallOptions, JoltTransport } from "./transport.js";
import type { IngressRecord, PublishedContent } from "./wire.js";

/** One record in the fake store. */
type StoredPublication = {
  body: unknown;
  seq: number;
  contentId: string;
  recipients: string[] | null;
};

/** An ingress send recorded by the fake. */
export type RecordedSend = {
  recipient: string;
  path: string;
  body: unknown;
};

/** App API behavior advertised by a deterministic fake daemon. */
export type FakeJoltOptions =
  | {
      featureDiscovery?: "advertised";
      appApi?: number;
      features?: Readonly<Record<string, number>>;
    }
  | {
      /** Model a reachable daemon on the exact Legacy App API v1 Baseline. */
      featureDiscovery: "legacy";
      appApi?: never;
      features?: never;
    };

/** Handle returned by {@link createFakeJolt}. */
export type FakeJolt = {
  /** The fake client; pass it anywhere a {@link JoltClient} (or any of its sub-interfaces) is expected. */
  client: JoltClient;
  /** The local identity the fake publishes under. */
  identity: string;
  /** Every object sent with `sendObject`, in order. */
  sent: RecordedSend[];
  /** Every encrypted publish's recipients, keyed by path. */
  encryptedRecipients: Map<string, string[]>;
  /**
   * Inject a pending ingress envelope, as if a remote sender delivered it.
   * Returns the created record so tests can accept/reject/open it.
   */
  deliverIngress(input: {
    sender: string;
    body: unknown;
    schemaHint?: string;
  }): IngressRecord;
};

let fakeCounter = 0;

/**
 * Create an in-memory fake Jolt for one identity.
 *
 * ```ts
 * const { client, deliverIngress, sent } = createFakeJolt("alice.jolt");
 * await client.publishJson("/myapp/profile", { name: "Alice" });
 * const got = await client.read(
 *   { identity: "alice.jolt", path: "/myapp/profile" },
 *   (v) => v as { name: string }
 * );
 * ```
 */
export function createFakeJolt(identity: string, options: FakeJoltOptions = {}): FakeJolt {
  const published = new Map<string, StoredPublication>();
  const tombstones = new Map<string, RecordDeletedResult>();
  const appends = new Map<string, StoredPublication[]>();
  const contentById = new Map<string, unknown>();
  const ingress = new Map<string, IngressRecord & { payload: unknown }>();
  const sent: RecordedSend[] = [];
  const encryptedRecipients = new Map<string, string[]>();
  const homeRelayPins = new Set<string>();
  const recordMutations = new Map<string, {
    operation: "update" | "delete" | "restore";
    result: RecordPresentResult | RecordDeletedResult;
  }>();
  const featureDiscovery = options.featureDiscovery ?? "advertised";
  const appApiFeatures = featureDiscovery === "legacy"
    ? { app_api: 1, features: {} }
    : { app_api: options.appApi ?? 1, features: { ...options.features } };

  function store(path: string, body: unknown, recipients: string[] | null): StoredPublication {
    const seq = (published.get(path)?.seq ?? -1) + 1;
    const record: StoredPublication = {
      body,
      seq,
      contentId: `cid_fake_${++fakeCounter}`,
      recipients,
    };
    published.set(path, record);
    tombstones.delete(path);
    contentById.set(record.contentId, body);
    return record;
  }

  function toResult(record: StoredPublication, path: string): PublishResult {
    return {
      contentId: record.contentId,
      latestSequence: record.seq,
      path,
      address: `${identity}${path}`,
    };
  }

  function publicationEntries(): Array<[string, StoredPublication]> {
    return [
      ...published.entries(),
      ...[...appends.entries()].flatMap(([path, records]) =>
        records.map((record): [string, StoredPublication] => [path, record])
      ),
    ];
  }

  function readStored<T>(
    ref: Reference,
    decode: Decoder<T>,
    encrypted: boolean
  ): Versioned<T> | null {
    if (ref.identity !== identity) return null;
    if (tombstones.has(ref.path)) return null;
    const record = published.get(ref.path);
    if (!record) return null;
    if (encrypted !== (record.recipients !== null)) return null;
    const value = decode(JSON.parse(JSON.stringify(record.body)));
    if (value === null) return null;
    return {
      ref,
      value,
      latestSequence: record.seq,
      contentId: record.contentId,
    };
  }

  const unusedTransport: JoltTransport = {
    async request(): Promise<never> {
      throw new Error("createFakeJolt has no transport; use the client methods directly.");
    },
    async upload(): Promise<never> {
      throw new Error("createFakeJolt has no transport; use the client methods directly.");
    },
  };

  const client: JoltClient = {
    transport: unusedTransport,

    async checkCompatibility(declaration: AppCompatibilityDeclaration) {
      return evaluateCompatibility(
        declaration,
        appApiFeatures,
        featureDiscovery
      );
    },

    async publishJson(path, body) {
      const record = store(path, body, null);
      return {
        ...toResult(record, path),
        revision: `revision_${record.seq}`,
      };
    },

    async resolve(ref) {
      if (ref.identity === identity && tombstones.has(ref.path)) {
        throw new JoltApiError("Path is tombstoned", {
          status: 410,
          code: "path_tombstoned",
        });
      }
      const record = ref.identity === identity ? published.get(ref.path) : undefined;
      if (record === undefined) {
        throw new JoltApiError("Path not found", {
          status: 404,
          code: "path_not_found",
        });
      }
      return {
        ref,
        latestSequence: record.seq,
        contentId: record.contentId,
      };
    },

    async read(ref, decode) {
      return readStored(ref, decode, false);
    },

    async readContent(contentId, ref, latestSequence, decode) {
      const body = contentById.get(contentId);
      if (body === undefined) return null;
      const value = decode(JSON.parse(JSON.stringify(body)));
      if (value === null) return null;
      return { ref, value, latestSequence, contentId };
    },

    async readRecord(ref) {
      if (ref.identity !== identity) return { state: "missing", ref };
      const tombstone = tombstones.get(ref.path);
      if (tombstone !== undefined) return tombstone;
      const record = published.get(ref.path);
      if (!record || record.recipients !== null) return { state: "missing", ref };
      return {
        state: "present",
        ref,
        contentId: record.contentId,
        revision: `revision_${record.seq}`,
        bytes: Array.from(new TextEncoder().encode(JSON.stringify(record.body))),
      };
    },

    async updateRecord(ref, body, mutation) {
      const retried = recordMutations.get(mutation.mutationId);
      if (retried !== undefined) {
        if (retried.operation !== "update" || retried.result.state !== "present") {
          throw new JoltApiError("Mutation ID was already used", {
            status: 400,
            code: "invalid_input",
          });
        }
        return retried.result;
      }
      const current = published.get(ref.path);
      if (
        ref.identity !== identity
        || current === undefined
        || tombstones.has(ref.path)
        || current.recipients !== null
        || mutation.revision !== `revision_${current.seq}`
      ) {
        throw new JoltApiError("Record revision changed", {
          status: 409,
          code: "record_conflict",
        });
      }
      const record = store(ref.path, body, null);
      const result = {
        state: "present" as const,
        ref,
        contentId: record.contentId,
        revision: `revision_${record.seq}`,
        bytes: Array.from(new TextEncoder().encode(JSON.stringify(record.body))),
      };
      recordMutations.set(mutation.mutationId, { operation: "update", result });
      return result;
    },

    async deleteRecord(ref, mutation) {
      const retried = recordMutations.get(mutation.mutationId);
      if (retried !== undefined) {
        if (retried.operation !== "delete" || retried.result.state !== "deleted") {
          throw new JoltApiError("Mutation ID was already used", {
            status: 400,
            code: "invalid_input",
          });
        }
        return retried.result;
      }
      const current = published.get(ref.path);
      if (
        ref.identity !== identity
        || current === undefined
        || tombstones.has(ref.path)
        || mutation.revision !== `revision_${current.seq}`
      ) {
        throw new JoltApiError("Record revision changed", {
          status: 409,
          code: "record_conflict",
        });
      }
      const result = {
        state: "deleted" as const,
        ref,
        revision: `revision_tombstone_${++fakeCounter}`,
      };
      tombstones.set(ref.path, result);
      recordMutations.set(mutation.mutationId, { operation: "delete", result });
      return result;
    },

    async restoreRecord(ref, body, mutation) {
      const retried = recordMutations.get(mutation.mutationId);
      if (retried !== undefined) {
        if (retried.operation !== "restore" || retried.result.state !== "present") {
          throw new JoltApiError("Mutation ID was already used", {
            status: 400,
            code: "invalid_input",
          });
        }
        return retried.result;
      }
      const tombstone = tombstones.get(ref.path);
      if (
        ref.identity !== identity
        || tombstone === undefined
        || mutation.revision !== tombstone.revision
      ) {
        throw new JoltApiError("Record revision changed", {
          status: 409,
          code: "record_conflict",
        });
      }
      const record = store(ref.path, body, null);
      const result = {
        state: "present" as const,
        ref,
        contentId: record.contentId,
        revision: `revision_${record.seq}`,
        bytes: Array.from(new TextEncoder().encode(JSON.stringify(record.body))),
      };
      recordMutations.set(mutation.mutationId, { operation: "restore", result });
      return result;
    },

    async publishAppend(path, body) {
      const record: StoredPublication = {
        body,
        seq: 0,
        contentId: `cid_fake_${++fakeCounter}`,
        recipients: null,
      };
      const list = appends.get(path) ?? [];
      list.push(record);
      appends.set(path, list);
      contentById.set(record.contentId, body);
      return toResult(record, path);
    },

    async enumerate(enumIdentity, pathPrefix) {
      if (enumIdentity !== identity) return [];
      const records: EnumeratedRecord[] = [];
      for (const [path, list] of appends) {
        if (!path.startsWith(pathPrefix)) continue;
        list.forEach((record, index) => {
          records.push({
            identity: enumIdentity,
            path,
            contentId: record.contentId,
            deviceId: "dev_fake",
            deviceSequence: index,
            createdAt: new Date(0).toISOString(),
            entryHash: `hash_${record.contentId}`,
          });
        });
      }
      return records;
    },

    async publishEncryptedJson(path, body, recipients) {
      encryptedRecipients.set(path, [...recipients]);
      return toResult(store(path, body, [...recipients]), path);
    },

    async readEncrypted(ref, decode) {
      return readStored(ref, decode, true);
    },

    async openEncrypted(target, path) {
      let match: [string, StoredPublication] | undefined;
      if (target.startsWith(`${identity}/`)) {
        const targetPath = target.slice(identity.length);
        const record = published.get(targetPath);
        if (record) match = [targetPath, record];
      } else if (path) {
        const record = published.get(path);
        if (record?.contentId === target) match = [path, record];
      }
      if (!match || match[1].recipients === null) {
        throw new Error(`encrypted publication not found: ${target}`);
      }
      const [openedPath, record] = match;
      const bytes = Array.from(new TextEncoder().encode(JSON.stringify(record.body)));
      return {
        contentId: record.contentId,
        path: openedPath,
        status: "decrypted",
        accessStatus: "available",
        bytes,
        size: bytes.length,
        contentType: "application/json",
        decryptError: null,
      };
    },

    async listPublished() {
      const items: PublishedContent[] = [];
      for (const [path, record] of publicationEntries()) {
        items.push({
          content_id: record.contentId,
          size: JSON.stringify(record.body).length,
          path,
          address: `${identity}${path}`,
          local_sequence: record.seq,
          pin_state: homeRelayPins.has(record.contentId) ? "relay_backed" : "local_only",
        });
      }
      return items;
    },

    async pinHomeRelay(contentId, path) {
      const match = publicationEntries().find(
        ([publishedPath, record]) =>
          record.contentId === contentId && (!path || publishedPath === path)
      );
      if (!match) {
        throw new Error(`published content not found: ${contentId}`);
      }
      const [, record] = match;
      homeRelayPins.add(contentId);
      return {
        status: "pinned",
        relay: "12D3KooWFakeRelay",
        owner: identity,
        contentId,
        latestSequence: record.seq,
        size: JSON.stringify(record.body).length,
      };
    },

    async sendObject(recipient, path, body) {
      sent.push({ recipient, path, body });
      return toResult(store(path, body, [recipient]), path);
    },

    async listPendingIngress() {
      return [...ingress.values()]
        .filter((record) => record.status === "pending")
        .map(({ payload: _payload, ...record }) => record);
    },

    async openIngress(ingressId) {
      const record = ingress.get(ingressId);
      return record ? JSON.parse(JSON.stringify(record.payload)) : null;
    },

    async acceptIngress(ingressId) {
      const record = ingress.get(ingressId);
      if (!record || record.status !== "pending") {
        throw new Error(`ingress envelope is not pending: ${ingressId}`);
      }
      record.status = "accepted";
    },

    async rejectIngress(ingressId) {
      const record = ingress.get(ingressId);
      if (!record || record.status !== "pending") {
        throw new Error(`ingress envelope is not pending: ${ingressId}`);
      }
      record.status = "rejected";
    },

    async requestSession() {
      return { request_id: "req_fake", status: "active" };
    },

    async getSessionRequestStatus(requestId) {
      return {
        request_id: requestId,
        session_token: "token_fake",
        status: "active",
        capabilities: [],
      };
    },

    async getCurrentSession() {
      return {
        request_id: "req_fake",
        app_id: "fake.app",
        app_name: "Fake",
        identity,
        granted_capabilities: [],
        status: "active",
      };
    },

    async getStatus() {
      return {
        daemon_version: "fake",
        peer_id: "12D3KooWFake",
        identity_address: identity,
        uptime_secs: 1,
        connected_peers: 0,
        direct_peers: 0,
        relayed_peers: 0,
        nat_type: "unknown",
        active_relays: 0,
        published_count: published.size,
        cached_count: 0,
        listen_addresses: [],
        bootstrap_relay: false,
        bootstrap_state: "idle",
        configured_bootstrap_relays: [],
        configured_bootstrap_relay_count: 0,
        effective_bootstrap_relays: [],
        effective_bootstrap_relay_count: 0,
        known_relay_count: 0,
        connected_bootstrap_peers: 0,
        last_bootstrap_error: null,
        home_relay: null,
      };
    },
  };

  return {
    client,
    identity,
    sent,
    encryptedRecipients,
    deliverIngress({ sender, body, schemaHint }) {
      const record: IngressRecord & { payload: unknown } = {
        ingress_id: `ing_fake_${++fakeCounter}`,
        receiver_id: "fake-live",
        sender_identity: sender,
        recipient_identity: identity,
        schema_hint: schemaHint ?? null,
        status: "pending",
        received_at: Math.floor(Date.now() / 1000),
        size: JSON.stringify(body).length,
        payload: body,
      };
      ingress.set(record.ingress_id, record);
      const { payload: _payload, ...wire } = record;
      return wire;
    },
  };
}

export { referenceKey };
