# 075: Recipient Ingress v0

**Type:** AFK after design  
**Milestone:** v0 Endgame  
**Status:** Implemented in PR
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

- [x] Alice cannot directly write Bob's update log or signed paths.
- [x] Incoming objects are encrypted for the recipient identity.
- [x] Incoming objects carry enough sender information to verify who sent them.
- [x] Bob can list pending incoming objects.
- [x] Bob can open/decrypt pending incoming objects through the daemon without
      exposing Bob's private keys to apps.
- [x] Bob can accept or reject incoming objects.
- [x] Rejected objects do not become Bob-owned signed state.
- [x] The API is generic and app-agnostic.
- [x] Spoke can use the primitive for replies/mentions.
- [x] Tests cover send, receive/open, accept, reject, and unauthorized access.

## Non-Goals

- Full inbox UI in Console.
- Protocol-level message/thread/feed semantics.
- Push notifications.
- Spam-resistant global public ingress.
- Offline relay storage markets.

## Notes

Implemented as the smallest direct/local ingress primitive:

- public direct receiver submission: `POST /api/v1/ingress`;
- app-session pending list: `GET /app/v1/ingress/pending`;
- app-session open/decrypt: `POST /app/v1/ingress/{ingress_id}/open`;
- app-session accept/reject:
  `POST /app/v1/ingress/{ingress_id}/accept` and
  `POST /app/v1/ingress/{ingress_id}/reject`;
- app-session identity-based send: `POST /app/v1/ingress/send`, which resolves
  the recipient's signed reachability record and submits to a live generic
  receiver endpoint;
- new app capabilities: `ingress:send`, `ingress:read`, and `ingress:decide`;
- daemon validates encrypted object envelope signature and local-recipient
  addressing before storing pending ingress;
- accepted/rejected ingress changes only local ingress status, not Bob's signed
  namespace.
- repeated same-status accept/reject decisions are idempotent, so a UI retry
  cannot strand an already-handled ingress as an application error.
- daemon `.jolt` resolution now returns verified cached answers immediately
  when available and refreshes known providers in the background, avoiding app
  UI stalls while keeping the protocol layer app-agnostic.

Current limits:

- pending ingress is daemon-local runtime state in this PR;
- relay-assisted offline buffering is still out of scope;
- Console does not grow an inbox UI;
- richer configurable abuse policy remains future hardening.

## Verification

- Red: `cargo test -p jolt-server test_recipient_ingress_submit_list_and_reject --test api_integration -- --nocapture`
  failed with 404 before ingress routes existed.
- Green: `cargo test -p jolt-server recipient_ingress --test api_integration -- --nocapture`.
- Green: `cargo test -p jolt-server test_app_can_submit_ingress_by_identity_reachability --test api_integration -- --nocapture`.
- Green: `cargo test -p jolt-network --lib daemon_resolution -- --nocapture`.
- Green: `npm test` in `apps/jolt-console`.
- Green: `cargo check -p jolt-network -p jolt-server`.
- Green: `./scripts/test-local.sh`.
