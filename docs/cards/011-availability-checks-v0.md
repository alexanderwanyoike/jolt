# 011: Availability Checks v0

**Type:** AFK  
**Milestone:** M4.5  
**Status:** Ready
**Blocked by:** None

## Why

Bob should not have to think about whether his relay is still serving his content. Bob's node should check that.

## What to Build

Add basic node-managed availability checks for home-relay-pinned content.

For v0, checking can be simple:

- Periodically ask the configured home relay for pinned content status.
- Optionally perform a fetch/verify probe for important pinned CIDs.
- Surface degraded availability in status/API output.

Do not implement automatic multi-relay repair yet unless it falls out naturally.

## Acceptance Criteria

- [ ] Node tracks which local published CIDs are expected to be pinned on home relay.
- [ ] Node can ask relay whether a CID is pinned.
- [ ] Node status/API reports healthy vs degraded relay availability.
- [ ] A failed relay check does not break local publishing/fetching.
- [ ] Tests cover healthy relay, missing pin, and unreachable relay.

## Notes

This is about transparency without burdening the user. Avoid complex policy language.
