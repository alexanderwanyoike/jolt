# Bidirectional Communication and Signed Reachability

## Status

Design proposal for card `058`.

## Problem

Private Pastey proves sender-owned encrypted sharing:

```text
Alice publishes encrypted bytes at Alice's signed path.
Bob resolves Alice's `.jolt` address and decrypts if authorized.
```

That is not enough for messaging, collaboration, calls, presence, or other
bidirectional apps. Those apps need to know:

- how to find a currently reachable peer;
- how to authenticate a live connection as a `.jolt` identity;
- how to deliver encrypted objects when the recipient is offline;
- how to reject abuse without accepting arbitrary writes;
- where contact, inbox, conversation, and application state belongs.

The protocol must not corrupt its current semantics to solve this. A remote
sender must not be able to mutate another identity's signed namespace.

## Decision

Jolt should own identity-rooted signed reachability primitives, not messaging semantics.

For v0, Jolt should provide:

- signed reachability endpoint records for an identity;
- identity-key and encryption-key discovery;
- authenticated daemon-to-daemon connection setup;
- optional generic encrypted streams for approved app sessions;
- optional recipient-controlled object ingress for offline delivery.

Jolt should not provide protocol-level inboxes, contacts, threads, read state,
spam folders, or message schemas. Those belong to apps or higher-level schemas
published as signed content.

This means Jolt can support messaging-like apps without becoming an email
protocol at the core.

## Protocol Boundary

Valid protocol-level statements:

```text
identity X signed path /.well-known/jolt/reachability -> CID Y at sequence N
CID Y is a signed reachability endpoint record for identity X.
identity X advertises endpoint E for protocol P until time T.
identity X accepts bounded object ingress under policy Q.
```

Invalid protocol-level statements:

```text
Alice sent Bob a message.
Bob has an inbox.
Alice is in Bob's contacts.
Message M is read.
Thread T has participants A, B, and C.
```

Apps can define those semantics using content schemas and local app state. The
protocol should only move authenticated, encrypted, bounded bytes.

## Signed Reachability Endpoint Records

Each identity may publish a signed reachability endpoint record under:

```text
/.well-known/jolt/reachability
```

This follows the existing `/.well-known/jolt/encryption-keys` pattern: it is a
reserved metadata path inside the identity owner's signed namespace, not an app
inbox and not a place remote senders can write.

Logical shape:

```json
{
  "type": "jolt.reachability",
  "version": 1,
  "identity": "bob_identity",
  "sequence_hint": 42,
  "issued_at": 1780000000,
  "expires_at": 1780003600,
  "live": [
    {
      "transport": "jolt-libp2p-stream",
      "peer_id": "12D3KooW...",
      "addresses": ["/ip4/203.0.113.10/udp/4100/quic-v1"],
      "relay_hints": ["relay_identity"],
      "protocols": ["opaque-app-stream-v1"],
      "max_payload_bytes": 1048576
    }
  ],
  "offline_ingress": [
    {
      "transport": "jolt-object-ingress-v1",
      "relay": "relay_identity",
      "endpoint": "opaque relay-local endpoint id",
      "requires_invite_token": true,
      "max_object_bytes": 65536,
      "max_objects_per_sender_per_hour": 20
    }
  ]
}
```

The exact wire type can be a core Rust struct later. The important contract is:

- the record is signed by the identity owner through the update log;
- endpoints expire quickly;
- endpoints describe capabilities and limits;
- endpoint payloads remain app-opaque;
- recipients can rotate or remove endpoints by publishing a newer record.

## Live Realtime Sessions

Jolt may provide a generic app stream:

```text
App A on Alice's daemon
  -> local app session capability check
  -> Alice daemon resolves Bob's reachability endpoint record
  -> Alice daemon opens authenticated encrypted stream to Bob daemon
  -> Bob daemon checks local receive policy
  -> opaque bytes flow between app handlers
```

The stream is identity-authenticated and transport-encrypted. Application
payload encryption may still be used for end-to-end object semantics, group
membership, transcript portability, and store-and-forward.

The daemon should know:

- local app session identity;
- remote `.jolt` identity;
- app/protocol identifier;
- byte limits and rate limits;
- whether the local user has allowed this app to initiate or receive streams.

The daemon should not know:

- whether the bytes are chat messages, collaboration operations, calls, game
  moves, or file-transfer control frames;
- contact lists;
- read/unread state;
- conversation membership.

Apps may choose another realtime transport if a reachability endpoint record
advertises one. Jolt's job is to sign and verify the endpoint metadata and bind
it to the identity. Jolt does not need to own every realtime transport.

## Offline Delivery

Offline delivery must not mean:

```text
Alice writes to bob.jolt/inbox
```

Only Bob can sign Bob's namespace.

Instead, offline delivery is recipient-controlled ingress:

```text
Bob publishes signed reachability metadata saying:
  "My daemon or chosen relay accepts bounded encrypted objects for me here,
   under this policy, until this expiry."

Alice submits an encrypted signed object to that endpoint.
The endpoint stores it as pending ingress, not as Bob's signed state.
Bob's daemon later pulls or receives it, verifies policy, decrypts if possible,
and hands it to an approved app.
The app decides whether it becomes an inbox item, notification, document
operation, game invite, or spam.
```

Ingress objects should be content-addressed and signed by the sender when the
sender identity is known:

```text
SignedIngressObject {
  sender_identity: Option<IdentityId>,
  recipient_identity: IdentityId,
  app_protocol: String,
  object_cid: ContentId,
  encrypted_object: EncryptedObjectEnvelope,
  issued_at: u64,
  expires_at: u64,
  nonce: bytes,
  invite_token_hash: Option<bytes>,
  sender_signature: Option<signature>
}
```

The relay or daemon can reject without understanding app payloads.

## Abuse Controls

Jolt cannot make DDoS impossible. It can avoid building an open unauthenticated
write target into the protocol.

Minimum controls:

- **No default open ingress:** an identity advertises ingress only when the user
  or daemon config allows it.
- **Short-lived endpoints:** reachability endpoint records and ingress
  endpoints expire.
- **Size limits:** maximum object bytes and stream frame bytes are advertised
  and enforced.
- **Queue limits:** per-recipient total bytes, object count, and age limits.
- **Sender limits:** per-sender and per-source rate limits.
- **Invite tokens:** unknown senders need a user/app-issued bearer token or
  equivalent capability before relay storage is accepted.
- **Allowlists:** known senders or signed relationship records can bypass some
  unknown-sender friction.
- **Unknown sender quarantine:** accepted unknown objects are not delivered to
  apps as trusted conversation state automatically.
- **Relay local policy:** relays can require authentication, payment, allowlist,
  proof-of-work, or operator-specific admission before accepting storage.
- **Backpressure:** live streams must be bounded by connection, bandwidth, and
  concurrent stream limits.

For direct attacks against Bob's current network address, normal network
defenses still apply: do not publish unnecessary direct addresses, prefer relay
hints when appropriate, rate-limit handshakes, and allow users to rotate
reachability endpoints.

## Relationship State

Relationship state belongs above the protocol layer.

Examples:

- contacts;
- follows;
- allowlists;
- blocklists;
- invite acceptance;
- sender reputation;
- moderation;
- notification settings.

The daemon may enforce local receive policy derived from that state, but the
state itself should be represented as app or user-controlled signed content.
The protocol should see only generic policy outcomes:

```text
accept stream
reject stream
accept ingress object
quarantine ingress object
drop ingress object
```

## How This Relates to Existing Docs

Older docs mention `/jolt/appsync/1.0.0` and `/jolt/message/1.0.0` as direct
protocols. Treat those as aspirational app-platform sketches, not current core
protocol commitments.

The direction from this design is smaller:

- no protocol-level direct-message primitive yet;
- no protocol-level app-sync semantics yet;
- first build signed reachability metadata;
- then build generic app-authorized streams or object ingress if product work
  needs them.

## Follow-Up Cards

Recommended implementation slices:

1. **Signed Reachability Endpoints v0:** core type, signed path publication,
   resolve API, and expiry validation for `/.well-known/jolt/reachability`.
2. **Daemon App Stream Capability v0:** session-scoped API for opening an
   authenticated opaque stream to a resolved remote identity and app protocol.
3. **Bounded Object Ingress Design:** deeper design for relay/daemon ingress,
   invite tokens, quotas, quarantine, and app delivery semantics.

Do not build yet:

- inboxes;
- contact systems;
- message schemas;
- read receipts;
- group chat;
- spam folders;
- public unauthenticated receive relays;
- global push-notification infrastructure.

## Open Questions

- Should the first implementation stop at signed reachability endpoint records
  before any stream API?
- Should object ingress require invite tokens from day one, even for known
  senders?
- Should receive policy live only in Console/daemon settings at first, or should
  apps be allowed to request scoped receive authority?
- Should Jolt define one generic app stream protocol, or only advertise
  endpoints for external transports until a concrete app needs streams?
