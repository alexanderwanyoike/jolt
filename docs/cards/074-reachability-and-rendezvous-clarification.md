# 074: Reachability and Rendezvous Clarification

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Ready after 073
**Blocked by:** 058, 069, 073

## Why

Reachability has been useful but easy to confuse with messaging. v0 needs clear
terms before two-way communication and Spoke build on it.

## Working Definitions

```text
Identity:
  Who this is cryptographically.

Reachability:
  Where/how this identity says it may be contacted.

Rendezvous:
  How two peers find each other for a live session when direct addresses are
  not enough.

App protocol:
  What the app does after contact is established.
```

Reachability finds the door. Two-way communication is what happens after
someone knocks.

## What to Decide

- Whether card 069's signed reachability record is enough for v0.
- Whether v0 needs an explicit rendezvous endpoint type.
- How live contact differs from offline ingress.
- What fields are protocol-level versus app-level.
- What Spoke needs from reachability/rendezvous and what can wait.

## Acceptance Criteria

- [ ] The terms identity, reachability, rendezvous, ingress, and app protocol
      are documented clearly.
- [ ] The v0 reachability fields required for two-way communication are listed.
- [ ] The design avoids app-level inbox/message/contact semantics.
- [ ] Spoke's minimum needs are identified.
- [ ] Unneeded rendezvous complexity is explicitly deferred.

## Non-Goals

- Building a generalized realtime framework.
- NAT traversal perfection.
- Global search/discovery.
- Messaging semantics in the protocol layer.

## Notes

If signed reachability is enough for v0, this card may close as a clarification
without new code. If not, split one narrow implementation card.
