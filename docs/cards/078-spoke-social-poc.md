# 078: Spoke Social PoC

**Type:** HITL then AFK  
**Milestone:** v0 Endgame  
**Status:** Ready after 073/075
**Blocked by:** 073, 075

## Why

Jolt needs one human-facing proof that can answer why someone would use it.
Spoke is the proposed small social app PoC that should pressure-test identity,
permissions, private sharing, optional pinning, and two-way communication.

## Product Bet

Spoke is not a Twitter clone. It is a small identity-owned social notebook for
known people.

Jolt should matter because:

- posts are signed under the user's identity;
- apps do not own the user's keys;
- replies/mentions use recipient-controlled ingress;
- content can be private/encrypted;
- availability can be delegated to relays without transferring ownership.

## Minimum Workflow

- Create or use local Jolt identity.
- Set a display name/profile for Spoke.
- Add a contact by `.jolt` identity or invite.
- Publish a post.
- Read posts from followed identities.
- Reply to a post through recipient ingress.
- Optionally create private posts/replies if the existing encryption path is
  practical.

## Acceptance Criteria

- [ ] Spoke uses Jolt app sessions and scoped capabilities.
- [ ] Spoke does not receive private keys.
- [ ] A user can publish a public post.
- [ ] A user can follow/read a known identity.
- [ ] A user can send a reply/mention through recipient ingress.
- [ ] A recipient can accept/reject incoming social objects.
- [ ] A local feed can be built from known/followed identities.
- [ ] Pinning is optional.
- [ ] Human demo works with at least two local identities/nodes.

## Non-Goals

- Global social network.
- Global search.
- Recommendations.
- Moderation system.
- App store.
- Protocol-level feed/contact/message semantics.

## Notes

Spoke should live outside the Jolt protocol repo unless there is a strong reason
to keep a tiny fixture here. Jolt protocol remains app-agnostic.
