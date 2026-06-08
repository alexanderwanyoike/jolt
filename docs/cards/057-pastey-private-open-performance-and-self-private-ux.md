# 057: Pastey Private Open Performance and Self-Private UX

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Implemented in PR  
**Blocked by:** 053

## Why

The Alice/Bob/Carol private Pastey demo works, but manual testing exposed two
rough edges:

- Opening a private paste can feel slow because Pastey and the daemon currently
  do redundant resolve/fetch work.
- Creating a private paste only for yourself feels awkward because Pastey
  requires at least one recipient and the user must manually add their own
  identity address.

Both issues make private Pastey feel less native than it should. Private notes
to yourself should be a first-class flow, and opening private content should not
pay for duplicate network-shaped work.

## What to Build

Improve private Pastey and the app encrypted APIs so that:

- Private open performs one resolve/fetch/decrypt flow instead of fetching
  ciphertext and then asking the daemon to resolve/fetch the same address again.
- Pastey supports self-only private pastes without making the user paste their
  own identity into the recipient list.
- User-facing errors remain visible long enough to read.

Possible daemon/API shapes:

- Add a single `POST /app/v1/encrypted/open` endpoint that resolves, fetches,
  checks path scope, decrypts if possible, and returns either plaintext or a
  ciphertext-fetched/decrypt-denied state.
- Or allow `POST /app/v1/encrypted/decrypt` to accept already-fetched encrypted
  bytes plus path/context, so Pastey can fetch once and decrypt once.

Choose the shape that keeps the protocol layer app-agnostic and keeps private
keys inside the daemon.

## Acceptance Criteria

- [x] Opening a private `.jolt` paste performs only one resolve and one fetch.
- [x] Bob opening a private paste he just sent does not refetch the same
      ciphertext through a second daemon call.
- [x] Carol opening Bob's private paste still fetches ciphertext but fails
      decrypt without seeing plaintext.
- [x] Pastey can create a self-only private paste without the user entering
      their own `.jolt` identity as a recipient.
- [x] Empty recipient input in private mode means "private to me" or an
      equivalent explicit UI state, not a confusing validation failure.
- [x] Private publish/open error messages remain visible and actionable.
- [x] Focused tests cover the app API behavior and Pastey UI behavior.

## Implementation Notes

- Jolt PR: `https://github.com/alexanderwanyoike/jolt/pull/74`.
- Pastey PR: `https://github.com/alexanderwanyoike/pastey/pull/3`.
- Added `POST /app/v1/encrypted/open` so apps can ask the daemon to resolve,
  fetch, path-check, and decrypt a private `.jolt` paste in one app API call.
- The endpoint returns `status: "decrypted"` with plaintext for recipients and
  `status: "ciphertext"` with ciphertext plus a decrypt error for non-recipients.
- Empty encrypted-publish recipient lists now mean self-only private content;
  the daemon already includes the local author encryption key in the recipient
  set.
- Pastey uses `/encrypted/open` for private opens, labels empty private
  recipient input as `private to me`, and keeps private publish/open errors
  visible in the page.

## Verification

- Red: `cargo test -p jolt-server test_app_can_encrypt_publish_self_only_private_content --test api_integration -- --nocapture` failed while empty encrypted recipients were rejected.
- Green: `cargo test -p jolt-server test_app_can_encrypt_publish_self_only_private_content --test api_integration -- --nocapture`.
- Red: `cargo test -p jolt-server test_app_ --test api_integration -- --nocapture` failed before `/app/v1/encrypted/open` existed.
- Green: `cargo test -p jolt-server test_app_ --test api_integration -- --nocapture`.
- Red: `npm test` in Pastey failed while `openPrivatePaste` was missing and empty private recipients were still rejected client-side.
- Green: `npm test` in Pastey.
- Green: `npm run build` in Pastey.
- Green: `./scripts/test-local.sh`.
- Manual: isolated local daemon on `127.0.0.1:9862`, Jolt Console Tauri
  approval, and Pastey on `127.0.0.1:5174`; self-only encrypted publish/open
  worked and private fetch felt fast.
- Green: automated three-daemon Alice/Bob/Carol app API smoke:
  Bob published an encrypted paste to Carol, Bob and Carol opened plaintext via
  `/app/v1/encrypted/open`, and Alice received only `status: "ciphertext"`.

## Notes

The current encrypted publish API includes the author as a recipient, which is
why self-opening works once the paste exists. The UX problem is that the publish
request currently requires at least one recipient before it reaches the daemon.

The performance issue is not crypto cost. It is redundant app/daemon API work:
Pastey fetches ciphertext first, then the daemon decrypt endpoint resolves and
fetches the same target again.
