# 078: Spoke Social PoC

**Type:** HITL then AFK  
**Milestone:** v0 Endgame  
**Status:** In Progress
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

## Implementation Notes

- Initial local PoC app lives at `/home/alexander/Code/Apps/jolt-apps/spoke`.
- Spoke uses app-owned JSON schemas only:
  - `spoke.profile.v1` at `/spoke/profile`;
  - `spoke.post.v1` at `/spoke/posts/{id}`;
  - `spoke.feed.v1` at `/spoke/feed`;
  - `spoke.reply.v1` for encrypted recipient ingress replies.
- The reply path uses existing daemon APIs without exposing private keys:
  Spoke encrypts and publishes an outgoing reply object under
  `/spoke/outgoing/{id}`, fetches the encrypted bytes by CID, and submits those
  bytes to the recipient daemon's public `/api/v1/ingress` endpoint.
- Pinning remains optional in this slice; the UI does not require a relay.
- Spoke now lives in the remote repository
  <https://github.com/alexanderwanyoike/spoke>.
- The first implementation PR was merged in that repository.
- Follow-up local verification exposed three product/UX fixes:
  - Spoke should poll feed and incoming state instead of relying on manual
    refresh.
  - The local object debug panel should not be part of the product UI.
  - Replies should appear in the relevant post thread for both recipient and
    sender.
- Sender-side thread visibility must not publish plaintext sent replies. Spoke
  should use the existing encrypted `/spoke/outgoing/{id}` object as the
  durable sent copy, because the encrypted publish API includes the sender's
  local identity as a recipient.
- Next product issue: Spoke should remove manual Receiver URL entry and send
  ingress by identity using Jolt reachability/rendezvous metadata.

## Verification

- Green: `npm test` in `/home/alexander/Code/Apps/jolt-apps/spoke`.
- Green: `npm run build` in `/home/alexander/Code/Apps/jolt-apps/spoke`.
- Green: `curl -sSf http://127.0.0.1:5178/` while the Spoke Vite dev server is
  running.
- Green: three local daemons for Alice/Bob/Carol were connected with two peers
  each, and the user verified post publishing, contact feed reading, reply
  submission, and accept/reject UI flows.
- Green: curl-level Bob-to-Alice ingress check encrypted a Spoke reply, submitted
  it to Alice's public ingress, opened plaintext through Alice's app-scoped API,
  and accepted it.
- Green: live Bob decrypt check opened Bob's own encrypted
  `/spoke/outgoing/reply_curl_e2e` through the app API, confirming sent replies
  can be shown without a public plaintext copy.
- Pending: remove manual Receiver URL from Spoke contact setup.
