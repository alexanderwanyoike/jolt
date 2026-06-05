# 058: Bidirectional Communication and Realtime Rendezvous Design

**Type:** HITL  
**Milestone:** Communication / App Platform Direction  
**Status:** Parked after 057  
**Blocked by:** 053, 057

## Why

Private Pastey proves sender-owned encrypted sharing:

```text
Alice publishes encrypted content at Alice's path.
Bob opens Alice's `.jolt` address and decrypts if authorized.
```

That is not the same as secure bidirectional communication. Messaging, email,
and realtime apps need answers for delivery, receiving, abuse control,
relationship state, and online sessions.

This is important for Jolt's product direction, but it is design-heavy and
should not derail the immediate Pastey polish in card 057.

## What to Decide

Define what Jolt should provide for bidirectional and realtime communication,
and what should stay above or outside the protocol.

Questions:

- Should Jolt only provide identity, signed state, encryption keys, relay hints,
  and peer discovery, while apps use another realtime transport?
- Does Jolt need a generic signed reachability/rendezvous record for live
  endpoints and supported protocols?
- Is offline delivery a daemon/app-layer receive queue rather than a protocol
  concept?
- How can users receive encrypted objects without letting remote senders write
  to their signed namespace?
- How should spam, DDoS, unknown senders, invite tokens, and contact policy be
  handled without making the protocol know about inboxes or contacts?
- What is the smallest design that enables messaging-like apps without
  corrupting the current protocol semantics?

## Non-Goals

- Do not add `inbox`, `messages`, contacts, threads, read state, or spam folders
  to the protocol layer.
- Do not let remote users write to another identity's signed paths.
- Do not implement a messaging app in this card.
- Do not commit to Jolt owning the realtime transport until alternatives are
  evaluated.

## Candidate Direction

Keep the protocol layer app-agnostic:

```text
identity X signed path P -> CID Y at sequence N
```

Use Jolt for:

- identity and key discovery;
- signed reachability/rendezvous metadata;
- encrypted signed object envelopes;
- relay availability and provider discovery;
- local daemon authority and app capability checks.

Let apps or higher layers define:

- inboxes;
- contacts and known senders;
- spam/quarantine;
- message schemas;
- conversation/thread state;
- read/unread state;
- realtime session semantics.

One possible model:

```text
Jolt resolves Bob's identity, public keys, and current rendezvous hints.
The messaging app uses those hints to establish a realtime channel over a
chosen transport, or submits encrypted signed objects to a bounded receive
queue advertised by Bob.
Bob's daemon pulls/ingests objects and the messaging app renders inbox state.
```

## Acceptance Criteria

- [ ] A design note or card update clearly separates Jolt protocol primitives
      from messaging/email app semantics.
- [ ] The design states whether Jolt should own realtime channels or only
      advertise/authenticate rendezvous information for other transports.
- [ ] Offline delivery is described without allowing senders to mutate a
      recipient's signed namespace.
- [ ] Abuse controls are considered: size limits, queue limits, rate limits,
      invite tokens, allowlists, unknown sender quarantine, and DDoS boundaries.
- [ ] Relationship/contact state is kept above the protocol layer.
- [ ] The output identifies one or more follow-up implementation cards, if any,
      and explicitly lists what should not be built yet.

## Notes

The current instinct is that Jolt should not become an email protocol at the
core. It should provide durable identity, trust, discovery, encryption, and
availability primitives. Messaging apps can use those primitives without
forcing `inbox` semantics into the network layer.
