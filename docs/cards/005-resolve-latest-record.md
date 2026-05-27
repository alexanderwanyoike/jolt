# 003: Resolve Latest Record by Identity

**Type:** AFK  
**Milestone:** M4  
**Status:** Blocked  
**Blocked by:** 004

## Why

After update logs exist, nodes need to resolve the latest valid state for a peer identity. This is what makes Jolt feel like a web instead of a CID fetcher.

## What to Build

Implement local latest-state resolution from a verified update log.

Given a peer's signed log entries, the node should derive:

- Latest root content CID.
- Latest profile metadata.
- Published logical paths mapped to content CIDs.
- Removed paths excluded from current state.

This card can stay local/in-memory. Network sync can come later.

## Acceptance Criteria

- [ ] Given a valid log, resolver returns the latest profile state.
- [ ] Given multiple updates to the same path, resolver returns the newest content CID.
- [ ] Given remove actions, resolver omits removed paths.
- [ ] Resolver rejects invalid or unverified logs.
- [ ] Tests cover replaying a realistic log into current state.

## Notes

This should avoid inventing app runtime concepts. The goal is a minimal mutable namespace.
