# 075: Recipient Ingress v0

**Type:** AFK after design  
**Milestone:** v0 Endgame  
**Status:** Blocked by 073 and 074
**Blocked by:** 073, 074

## Why

Spoke needs a way for Alice to send Bob a reply or mention without Alice
writing to Bob's namespace. Recipient ingress is the smallest generic primitive
that supports this while preserving identity-owned state.

## What to Build

Implement the v0 recipient-controlled ingress path:

- Bob publishes an ingress-capable reachability record.
- Alice submits an encrypted object addressed to Bob.
- Bob's daemon receives or fetches the object.
- Bob's daemon exposes pending incoming objects to local/admin/app surfaces.
- Bob or an authorized app accepts/rejects an object.
- Accepted objects can be signed into Bob-owned application state by an app
  with the right capability.

## Acceptance Criteria

- [ ] Alice cannot directly write Bob's update log or signed paths.
- [ ] Incoming objects are encrypted for the recipient identity.
- [ ] Incoming objects carry enough sender information to verify who sent them.
- [ ] Bob can list pending incoming objects.
- [ ] Bob can accept or reject incoming objects.
- [ ] Rejected objects do not become Bob-owned signed state.
- [ ] The API is generic and app-agnostic.
- [ ] Spoke can use the primitive for replies/mentions.
- [ ] Tests cover send, receive, accept, reject, and unauthorized access.

## Non-Goals

- Full inbox UI in Console.
- Protocol-level message/thread/feed semantics.
- Push notifications.
- Spam-resistant global public ingress.
- Offline relay storage markets.

## Notes

This card should be implemented only after card 073 settles the semantics.
