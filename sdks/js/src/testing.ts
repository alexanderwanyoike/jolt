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
  Reference,
  Versioned,
} from "./client.js";
import { referenceKey } from "./client.js";
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
export function createFakeJolt(identity: string): FakeJolt {
  const published = new Map<string, StoredPublication>();
  const appends = new Map<string, StoredPublication[]>();
  const contentById = new Map<string, unknown>();
  const ingress = new Map<string, IngressRecord & { payload: unknown }>();
  const sent: RecordedSend[] = [];
  const encryptedRecipients = new Map<string, string[]>();

  function store(path: string, body: unknown, recipients: string[] | null): StoredPublication {
    const seq = (published.get(path)?.seq ?? -1) + 1;
    const record: StoredPublication = {
      body,
      seq,
      contentId: `cid_fake_${++fakeCounter}`,
      recipients,
    };
    published.set(path, record);
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

  function readStored<T>(
    ref: Reference,
    decode: Decoder<T>,
    encrypted: boolean
  ): Versioned<T> | null {
    if (ref.identity !== identity) return null;
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

    async publishJson(path, body) {
      return toResult(store(path, body, null), path);
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

    async listPublished() {
      const items: PublishedContent[] = [];
      for (const [path, record] of published) {
        items.push({
          content_id: record.contentId,
          size: JSON.stringify(record.body).length,
          path,
          address: `${identity}${path}`,
          local_sequence: record.seq,
          pin_state: "pinned",
        });
      }
      return items;
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
        peer_id: "12D3KooWFake",
        identity_address: identity,
        uptime_secs: 1,
        connected_peers: 0,
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
