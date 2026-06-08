# 076: Optional and Authorized Relay Pinning

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready after 072
**Blocked by:** 072

## Why

Pinning is useful availability delegation, but it must not be required for Jolt
to work. A user should be able to publish and communicate without configuring a
home relay. Relays also need explicit policy; arbitrary users should not be able
to fill any relay unless that relay intentionally accepts public pins.

## What to Build

Make the v0 relay pinning story explicit and enforced:

- publishing works without pinning;
- pinning is an optional action or setting;
- relay pin endpoints enforce relay-side policy;
- relays can reject public pins by default;
- relays can allowlist identities;
- relays can explicitly enable public pinning with bounds;
- errors explain whether pinning is unavailable, unauthorized, or rejected by
  policy.

## Acceptance Criteria

- [ ] No normal publish/fetch/private-sharing flow requires pinning.
- [ ] Relay pinning defaults are conservative.
- [ ] Relay can reject pins from unauthorized identities.
- [ ] Relay can allow configured identities.
- [ ] Public pinning requires explicit relay configuration.
- [ ] Pin rejection errors are structured and user-readable.
- [ ] Existing home-relay pin flows continue to work when authorized.
- [ ] Tests cover unauthorized, allowlisted, and public-pinning cases.

## Non-Goals

- Payments.
- Storage markets.
- Quota marketplace.
- Public relay reputation.

## Notes

Pinning is about availability, not ownership. Owner-signed paths remain the
source of truth.
