# 044: Capability-Checked App API v0

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Done
**Blocked by:** 043

## Why

The current daemon API is trusted/debug-oriented. External apps need a separate app-facing API that checks a session token and capability set before publishing, fetching, resolving, pinning, or listing content.

## What to Build

Add `/app/v1/*` equivalents for the operations Pastey needs:

- Resolve public `.jolt` addresses.
- Fetch public content.
- Publish under an approved path prefix.
- List published inventory under an approved path prefix.
- Pin own published content when the session allows it.

Every app endpoint must:

- Require a valid session token.
- Check the requested operation against session capabilities.
- Reject path escapes outside the granted prefix.
- Avoid exposing admin-only daemon state.

## Acceptance Criteria

- [x] App resolve requires `resolve:public` or equivalent.
- [x] App fetch requires `fetch:public` or equivalent.
- [x] App publish requires a granted identity and path prefix such as `/pastes/*`.
- [x] App inventory filters to the granted path prefix.
- [x] App pin only allows own published content within the granted path prefix.
- [x] Admin/debug `/api/v1/*` behavior remains available for now.
- [x] Tests cover allowed and denied operations.

## Result

- Added capability-checked app endpoints for resolve, fetch, publish, published inventory, and home-relay pin requests.
- App endpoints require a valid bearer app-session token.
- Public reads require `resolve:public` or `fetch:public`.
- Own writes require the session identity to match the local daemon identity.
- Path-scoped operations use granted path capabilities such as `publish:/pastes/*`, `inventory:/pastes/*`, and `pin:own:/pastes/*`.
- App publish rejects path escapes before delegating to the daemon.
- Trusted/debug `/api/v1/*` endpoints remain unchanged.
- Added integration tests for allowed and denied app operations.

## Notes

The network layer should not know about app sessions. Capability checks happen before the daemon performs a network operation.
