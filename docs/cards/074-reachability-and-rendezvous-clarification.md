# 074: Reachability and Rendezvous Clarification

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Clarified in PR
**Blocked by:** 058, 069, 073

## Why

Reachability has been useful but easy to confuse with messaging. v0 needs clear
terms before two-way communication and Spoke build on it.

This card clarifies what Jolt needs for v0 and what should wait.

## Decision

Card 069's signed reachability record is the right foundation for v0.

v0 does not need a generalized rendezvous framework. It needs direct-first
receiver discovery and optional relay-assisted offline delivery.

```text
Reachability finds a door.
Rendezvous helps peers find or approach the door.
Ingress is the controlled knock.
The app decides what the knock means.
```

## Terms

```text
Identity:
  Who this is cryptographically.

Reachability:
  Signed identity-owned metadata describing where/how this identity may be
  contacted for a bounded time.

Rendezvous:
  Peer-finding assistance when direct advertised addresses are insufficient.
  In v0 this means relay hints or relay-assisted discovery, not a separate
  messaging layer.

Receiver:
  A reachability endpoint that accepts generic signed encrypted ingress
  envelopes under recipient policy.

Ingress:
  Recipient-controlled submission of generic signed encrypted envelopes.
  Ingress may be direct/live or relay-assisted/offline.

App protocol:
  What an app does with contact once established or with an accepted ingress
  envelope.
```

Protocol statements stay generic:

```text
Bob signed a reachability record.
Bob advertises a direct receiver endpoint until time T.
Bob advertises an optional relay-assisted receiver endpoint until time T.
Bob accepts bounded encrypted ingress under policy P.
Alice submitted a signed encrypted envelope to Bob's receiver.
```

Application statements stay above the protocol:

```text
Alice replied to Bob.
Bob has an inbox.
Carol mentioned Bob.
Alice is Bob's contact.
Spoke shows an unread notification.
```

## v0 Reachability Requirements

For recipient-controlled ingress, Bob's reachability record must be able to
describe:

- identity that signed the record;
- issued and expires timestamps;
- direct/live receiver endpoints;
- optional relay-assisted offline receiver endpoints;
- endpoint id;
- transport or endpoint kind;
- address or relay/capability reference;
- accepted schema hints;
- max envelope bytes;
- max payload bytes;
- required encryption suites;
- sender signature requirement;
- expiry window;
- unknown-sender policy;
- rate-limit policy hints;
- pending queue policy hints;
- relay authorization requirements for offline buffering.

These fields are protocol-level because they describe routing, verification,
resource limits, and recipient safety. They must not describe app concepts such
as inboxes, messages, contacts, posts, feeds, or notifications.

## Direct-First Rule

Direct/live delivery is the preferred path:

```text
Alice resolves Bob's signed reachability.
Alice finds a live receiver endpoint.
Alice submits a signed encrypted ingress envelope directly to Bob's node.
Bob's node validates, rejects, or stores pending ingress locally.
```

If direct delivery works, no relay is needed.

Direct delivery may use explicit addresses, local/LAN addresses, peer addresses,
or other future transport-specific endpoint references. The reachability record
binds those endpoint hints to Bob's identity.

## Optional Relay-Assisted Delivery

Relay-assisted delivery is an optional fallback:

```text
Alice resolves Bob's signed reachability.
Bob has no reachable live receiver, or Alice cannot connect directly.
Bob advertises an offline receiver backed by an authorized relay buffer.
Alice submits a signed encrypted ingress envelope to that relay endpoint.
Bob's node fetches or receives it later.
```

The relay is:

- a temporary encrypted delivery buffer;
- subject to Bob's receiver policy;
- subject to relay operator policy;
- optional;
- best-effort.

The relay is not:

- Bob's source of truth;
- Bob's signed namespace;
- a protocol inbox;
- required infrastructure for two-way communication.

If Bob advertises no usable direct endpoint and no usable offline receiver,
Alice receives `receiver_unavailable`.

## Rendezvous in v0

For v0, rendezvous means:

```text
help Alice discover or approach Bob's live receiver endpoint
```

Acceptable v0 rendezvous mechanisms:

- relay hints in signed reachability;
- existing relay mesh/provider discovery for finding identity metadata;
- transport-specific peer/address hints already represented in reachability;
- optional relay-assisted offline receiver endpoints.

Deferred:

- generalized rendezvous protocol;
- NAT traversal perfection;
- hole punching product work;
- always-on realtime presence;
- global user search;
- contact discovery;
- app-level matchmaking semantics.

If Spoke needs matchmaking later, that should be an app-level or future
transport-specific design, not a prerequisite for v0 ingress.

## Spoke Minimum Needs

Spoke v0 needs only:

- resolve a target identity's reachability;
- find a direct receiver if available;
- fall back to optional relay-assisted receiver if advertised;
- submit a signed encrypted ingress envelope with a Spoke schema hint;
- receive generic delivery/policy errors;
- let Bob's node/app accept or reject before publishing Bob-owned state.

Spoke does not need:

- protocol contacts;
- protocol inboxes;
- protocol feeds;
- global search;
- realtime presence;
- perfect NAT traversal;
- relay-required delivery.

## App Boundary

Jolt should expose reachability and ingress as generic transport/control
primitives. Apps own user-facing meaning.

Jolt may say:

```text
receiver found
receiver unavailable
relay fallback available
ingress queued
ingress rejected by policy
```

Spoke may say:

```text
reply sent
mention pending
unknown sender replied
post interaction rejected
```

The protocol must not need to understand those Spoke phrases.

## Acceptance Criteria

- [x] The terms identity, reachability, rendezvous, ingress, and app protocol
      are documented clearly.
- [x] The v0 reachability fields required for two-way communication are listed.
- [x] The design avoids app-level inbox/message/contact semantics.
- [x] Spoke's minimum needs are identified.
- [x] Unneeded rendezvous complexity is explicitly deferred.

## Non-Goals

- Building a generalized realtime framework.
- NAT traversal perfection.
- Global search/discovery.
- Messaging semantics in the protocol layer.
- Protocol contacts, inboxes, feeds, notifications, or matchmaking.
- Requiring relays for two-way communication.

## Follow-Up

Implementation belongs in [075](075-recipient-ingress-v0.md). That card should
use the existing signed reachability foundation and add only the minimum
receiver/ingress support needed for Spoke.

If implementation reveals that card 069's reachability record shape lacks one
of the required generic receiver fields above, add that field as part of the
smallest 075 slice. Do not introduce a separate rendezvous subsystem for v0.

## Verification

Docs-only clarification. No code tests were run.
