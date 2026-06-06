# 069: Signed Reachability Endpoints v0

**Type:** AFK
**Milestone:** Communication / App Platform Direction
**Status:** Implemented in PR
**Blocked by:** 058

## Why

Bidirectional and realtime apps need a way to discover how an identity can be
reached without adding inbox, message, or contact semantics to the protocol.

The first safe slice is signed reachability metadata: Bob signs where Bob can be
reached and which generic protocols are supported. Alice can verify that
metadata before trying a live connection or a later offline-ingress path.

## What to Build

Implement the first protocol/data slice from
[Bidirectional Communication and Signed Reachability](../19-signed-reachability-endpoints.md):

- Core type for a v0 signed reachability endpoint record.
- Reserved signed path `/.well-known/jolt/reachability`.
- Validation for owner identity, supported version, endpoint shape, and expiry.
- Publish/update API or daemon command for the local identity's current
  reachability endpoint record.
- Resolve/read API for another identity's verified reachability endpoint record.

Keep endpoint payloads generic and app-agnostic. Do not add messaging or inbox
semantics.

## Acceptance Criteria

- [x] A reachability endpoint record can be signed and verified for the owning identity.
- [x] Expired reachability endpoint records are rejected or returned as unusable.
- [x] A daemon can publish its local reachability endpoint record under
  `/.well-known/jolt/reachability`.
- [x] A resolver/API can fetch and verify another identity's reachability endpoint record.
- [x] Invalid owner signatures, wrong identities, unsupported versions, and
  malformed endpoints are covered by tests.
- [x] The implementation does not add inbox, contact, thread, message, or app
  schema concepts to protocol code.

## Verification Notes

- Added protocol/core type `ReachabilityRecord` plus live and offline-ingress
  endpoint structs.
- Added reserved signed path `/.well-known/jolt/reachability`.
- Added `POST /admin/v1/reachability` to publish the local identity's signed
  reachability endpoint record.
- Added `GET /api/v1/identities/{identity}/reachability` to resolve, fetch, and
  verify another identity's signed reachability endpoint record.
- Verified with:
  - `cargo test -p jolt-core reachability -- --nocapture`
  - `cargo test -p jolt-server test_admin_can_publish_and_api_can_verify_signed_reachability --test api_integration -- --nocapture`

## Notes

This card should stop before live app streams and offline object ingress.
Those need separate implementation/design cards after the reachability endpoint
record shape is proven.
