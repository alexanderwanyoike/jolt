# 044: Capability-Checked App API v0

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Ready after 043  
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

- [ ] App resolve requires `resolve:public` or equivalent.
- [ ] App fetch requires `fetch:public` or equivalent.
- [ ] App publish requires a granted identity and path prefix such as `/pastes/*`.
- [ ] App inventory filters to the granted path prefix.
- [ ] App pin only allows own published content within the granted path prefix.
- [ ] Admin/debug `/api/v1/*` behavior remains available for now.
- [ ] Tests cover allowed and denied operations.

## Notes

The network layer should not know about app sessions. Capability checks happen before the daemon performs a network operation.
