# 073: Two-Way Communication Design

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Ready after 072
**Blocked by:** 058, 069, 072

## Why

Jolt currently works well for identity-owned publishing and reads. Alice can
read Bob's signed paths, and Alice can publish Alice's signed paths. But Jolt
does not yet support a safe way for Alice to send something to Bob.

Two-way communication is required before Spoke can be a meaningful social PoC.

## Design Principle

Alice must not write Bob's namespace.

Instead:

```text
Bob publishes how Bob can receive.
Alice sends an encrypted object to Bob's receiver.
Bob validates, accepts, rejects, or ignores it.
If accepted, Bob signs Bob-owned state.
```

## What to Design

Define the v0 recipient-controlled communication model:

- receiver discovery through signed reachability;
- live session versus offline ingress;
- encrypted object submission;
- sender identity and signature expectations;
- recipient-side accept/reject policy;
- spam/rate-limit hooks;
- how accepted objects become recipient-owned signed state;
- what errors an app sees.

## Acceptance Criteria

- [ ] The design preserves identity-owned namespaces.
- [ ] The protocol layer remains app-agnostic.
- [ ] Alice cannot directly mutate Bob's update log.
- [ ] Bob can publish whether/how Bob accepts incoming objects.
- [ ] Bob can accept/reject incoming objects before they become Bob-owned state.
- [ ] The design is sufficient for Spoke replies/mentions.
- [ ] The design does not introduce protocol-level inbox/message/contact/feed
      semantics.

## Non-Goals

- Full messaging product.
- Global inbox.
- Contacts/social graph in protocol.
- Read receipts, typing indicators, threads, moderation UI, or notifications.
- Public unauthenticated write access to arbitrary relays.

## Notes

This card should decide semantics before implementation. Implementation should
move to card 075 only after the accept/reject flow is clear.
