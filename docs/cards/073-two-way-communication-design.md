# 073: Two-Way Communication Design

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Designed in PR
**Blocked by:** 058, 069, 072

## Why

Jolt currently works well for identity-owned publishing and reads. Alice can
read Bob's signed paths, and Alice can publish Alice's signed paths. But Jolt
does not yet support a safe way for Alice to send something to Bob.

Two-way communication is required before Spoke can be a meaningful social PoC.

## Decision

Jolt v0 will support recipient-controlled ingress.

The core rule is:

```text
Alice must not write Bob's namespace.
```

Instead:

```text
Bob publishes how Bob can receive.
Alice submits a signed encrypted ingress envelope to Bob's receiver.
Bob's node validates the envelope against generic receiver policy.
Bob's node stores accepted-for-review envelopes as local pending ingress.
Bob or Bob's app accepts, rejects, ignores, or blocks.
Only accepted objects become Bob-owned signed state.
```

This gives apps two-way communication without protocol-level inbox, message,
contact, profile, feed, thread, or social semantics.

## Terms

- **Receiver:** Bob-controlled endpoint metadata, published through signed
  reachability, describing how Bob accepts incoming envelopes.
- **Ingress envelope:** Generic signed encrypted object submitted by one
  identity to another identity.
- **Pending ingress:** Locally persisted receiver-owned queue/state for envelopes
  that passed generic checks but have not become accepted recipient-owned state.
- **Accepted state:** Bob-owned signed paths or content that Bob publishes after
  accepting an envelope.
- **Relay buffer:** Optional offline delivery helper. It is not the source of
  truth and is not required for two-way communication.

## Delivery Model

Direct/live delivery is primary:

```text
Alice -> Bob's node
```

Alice resolves Bob's signed reachability record, finds a live receiver endpoint,
and submits the encrypted ingress envelope directly. Bob's node validates,
rejects, or stores pending ingress locally.

Relay-assisted offline delivery is optional:

```text
Alice -> authorized relay buffer -> Bob's node later
```

Bob may advertise an offline receiver backed by a relay that is willing to store
encrypted envelopes temporarily. The relay is a best-effort buffer, not required
infrastructure. If Bob has no reachable live endpoint and no offline receiver,
Alice receives `receiver_unavailable`.

Relays must not be required for local, LAN, or otherwise directly reachable
communication.

## Persistence

Persistence is intentionally split:

```text
relay = temporary encrypted delivery buffer
Bob's node = durable pending ingress store
Bob's signed namespace = accepted state only
```

The relay may hold encrypted envelopes for offline delivery, subject to relay
policy and expiry. Once Bob's node receives an envelope and it passes generic
receiver checks, Bob's node persists it locally as pending ingress.

No incoming envelope becomes part of Bob's signed namespace until Bob or an
authorized Bob app accepts it and Bob signs/publishes recipient-owned state.

## Receiver Discovery

Bob advertises receiver metadata through signed reachability. The protocol data
must remain generic. A receiver may include:

- receiver id;
- endpoint type, such as direct live endpoint or relay-assisted offline buffer;
- endpoint address/capability reference;
- accepted schema hints;
- maximum envelope bytes;
- maximum payload bytes;
- required encryption suites;
- expiry window;
- sender signature requirement;
- unknown-sender policy;
- rate-limit policy hints;
- pending queue limits;
- relay authorization requirements for offline buffering.

The receiver metadata does not define app concepts. `accepted_schema_hints` are
opaque identifiers that let apps and receivers filter unsupported object types
without Jolt understanding the object meaning.

## Ingress Envelope

The v0 envelope should be generic and signed by the sender. It should contain:

- envelope id or nonce for replay/idempotency handling;
- sender identity;
- recipient identity;
- receiver id, if the reachability record exposes multiple receivers;
- created and expires timestamps;
- schema hint;
- encrypted payload reference or inline encrypted payload;
- encryption suite metadata;
- optional reply/context references as opaque app data;
- sender signature over the envelope.

The payload must be encrypted for the recipient. The protocol may expose the
schema hint and envelope metadata before decryption, but application meaning
comes from the decrypted payload and the app that understands that schema.

## Receiver Policy and Abuse Handling

Bob controls receiver policy. v0 must include configurable generic abuse
controls before Spoke uses the primitive:

- `max_envelope_bytes`;
- `max_payload_bytes`;
- `max_pending_per_sender`;
- `max_pending_total`;
- per-sender rate limits;
- global rate limits;
- accepted schema hints;
- allow/reject unknown senders;
- blocked senders;
- required sender signature;
- required encryption;
- expiry window.

Default v0 policy should be conservative:

- require sender signature;
- require encryption;
- keep unknown senders pending rather than auto-accepted;
- enforce size limits;
- enforce rate limits;
- require expiry;
- reject blocked senders before storage.

Jolt enforces envelope/resource safety. Apps enforce product meaning and social
policy.

## Recipient State Machine

An incoming envelope moves through generic states:

```text
submitted
validated
rejected
pending
accepted
ignored
blocked
expired
```

Important distinctions:

```text
receive != accept
pending != accepted recipient-owned state
```

Bob's node may reject at the door for invalid signatures, unsupported schemas,
oversized payloads, expiry, rate limits, blocked senders, encryption failures,
or receiver policy.

If an envelope is valid but needs review, it becomes pending local ingress.

If Bob or an authorized app accepts it, the app may publish recipient-owned state
using Bob's normal signed publishing path. For Spoke, that might mean Bob
publishes a Bob-owned accepted reply/mention record. Jolt does not know that the
object is a reply or mention.

## App Boundary

The protocol layer knows:

```text
signed encrypted envelope
sender identity
recipient identity
schema hint
receiver policy status
pending/accepted/rejected lifecycle
```

The app layer knows:

```text
reply
mention
private paste share
social request
notification
conversation
```

Jolt APIs may expose generic pending ingress records and generic actions such as
accept, reject, ignore, or block. App UI owns user-facing language and
interpretation. Console may show generic local trust/control information, but
Console must not become a social inbox.

## App-Visible Errors

Apps submitting ingress should receive generic errors:

- `delivered`;
- `queued`;
- `receiver_unavailable`;
- `receiver_not_found`;
- `rejected_by_policy`;
- `unknown_sender_rejected`;
- `blocked_sender`;
- `unsupported_schema`;
- `too_large`;
- `rate_limited`;
- `pending_limit_exceeded`;
- `expired`;
- `invalid_signature`;
- `encryption_required`;
- `relay_not_authorized`;
- `relay_unavailable`;
- `unreachable`.

These errors describe transport and receiver policy, not app semantics.

## Spoke v0 Fit

This design is sufficient for Spoke because:

- Alice can submit a reply/mention object to Bob without writing Bob's namespace;
- Bob can reject or ignore unknown senders;
- Bob can accept valid Spoke objects into Bob-owned signed state;
- Spoke can display app-specific pending social interactions;
- Jolt remains generic.

## Acceptance Criteria

- [x] The design preserves identity-owned namespaces.
- [x] The protocol layer remains app-agnostic.
- [x] Alice cannot directly mutate Bob's update log.
- [x] Bob can publish whether/how Bob accepts incoming objects.
- [x] Bob can accept/reject incoming objects before they become Bob-owned state.
- [x] The design is sufficient for Spoke replies/mentions.
- [x] The design does not introduce protocol-level inbox/message/contact/feed
      semantics.

## Non-Goals

- Full messaging product.
- Global inbox.
- Contacts/social graph in protocol.
- Read receipts, typing indicators, threads, moderation UI, or notifications.
- Public unauthenticated write access to arbitrary relays.
- Requiring relays for two-way communication.
- App-specific receiver semantics in the protocol layer.

## Follow-Up

Implementation belongs in [075](075-recipient-ingress-v0.md). That card should
build the smallest generic recipient ingress primitive needed for Spoke without
adding app-level messaging/social concepts to the protocol layer.

## Verification

Docs-only design decision. No code tests were run.
