# 004: Profile and Feed v0

**Type:** AFK  
**Milestone:** M4  
**Status:** Blocked  
**Blocked by:** 005

## Why

The simplest product slice for mutable content is a profile/feed. It demonstrates user-owned mutable web presence without needing WASM apps.

## What to Build

Add a minimal profile/feed model on top of update logs.

A user should be able to publish:

- Display name.
- Bio.
- Optional avatar CID.
- Feed entries pointing to content CIDs.

Another node should be able to resolve that state from the user's signed log.

## Acceptance Criteria

- [ ] A profile update can be represented in the update log.
- [ ] A feed item can point to a content CID.
- [ ] Current profile/feed state can be resolved from signed entries.
- [ ] Tests show publish profile v1, update profile v2, publish two feed items, resolve latest state.
- [ ] Docs show a simple Alice/Bob profile/feed flow.

## Notes

This is intentionally not a social network. It is a protocol proof for mutable user-owned state.
