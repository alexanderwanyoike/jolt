# 078: Spoke Social PoC

**Type:** HITL then AFK  
**Milestone:** v0 Endgame  
**Status:** In Progress / PRs Open
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

- [x] Spoke uses Jolt app sessions and scoped capabilities.
- [x] Spoke does not receive private keys.
- [x] A user can publish a public post.
- [x] A user can follow/read a known identity.
- [x] A user can send a reply/mention through recipient ingress.
- [x] A recipient can accept/reject incoming social objects.
- [x] A local feed can be built from known/followed identities.
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
  `/spoke/outgoing/{id}`, fetches the encrypted bytes by CID, and asks Jolt to
  submit those bytes through app-scoped `POST /app/v1/ingress/send`.
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
- Manual Receiver URL entry has been removed. Contacts are identified by Jolt
  identity, and Jolt resolves signed reachability for reply submission.
- Spoke feed indexes now include immutable post CIDs. Readers resolve
  `/spoke/feed` once and fetch post CIDs directly when available, avoiding the
  previous recursive/N+1 `.jolt` resolution pattern for migrated entries.
- Remaining product issue: Spoke still polls instead of subscribing to daemon
  state/events. The app works, but the local app/daemon interface needs a
  future evented/materialized-view shape before it will feel native.

## Verification

- Green: `npm test` in `/home/alexander/Code/Apps/jolt-apps/spoke`.
- Green: `npm run build` in `/home/alexander/Code/Apps/jolt-apps/spoke`.
- Green: `npm test -- api.test.ts feed.test.ts` in
  `/home/alexander/Code/Apps/jolt-apps/spoke`.
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
- Green: live read-only diagnosis showed remote `.jolt` feed/post resolution was
  causing recursive/N+1 delay; Spoke now stores post `contentId` in feed entries
  and Jolt returns cached verified resolve results immediately while refreshing
  in the background.
