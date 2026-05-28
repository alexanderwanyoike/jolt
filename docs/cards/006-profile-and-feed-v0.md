# 006: Space and Feed v0

**Type:** AFK  
**Milestone:** M4 / Community substrate
**Status:** Ready
**Blocked by:** 005

## Why

The simplest product slice for mutable content is a signed space with a feed.

This demonstrates the new core thesis: Jolt is not a file browser or generic web clone. A user or community identity owns a space, publishes signed updates into it, and grants other identities access to the parts they should see.

## What to Build

Add a minimal space/feed model on top of update logs.

A space should be able to publish:

- Title.
- Description.
- Optional avatar CID.
- Feed entries pointing to content CIDs.
- A visibility marker for v0: public or members-only.

Another node should be able to resolve that state from the identity's signed log.

## Acceptance Criteria

- [ ] A space metadata update can be represented in the update log.
- [ ] A feed item can point to a content CID.
- [ ] Current space/feed state can be resolved from signed entries.
- [ ] Tests show publish space v1, update space v2, publish two feed items, resolve latest state.
- [ ] Docs show a simple Alice/Bob space/feed flow.

## Notes

This is intentionally not a social network. It is a protocol proof for mutable user-owned or community-owned state.
