# 006: Signed Path Publishing v0

**Type:** AFK  
**Milestone:** M4 / Mutable identity namespace
**Status:** Done
**Blocked by:** 005

## Why

Jolt addresses should be usable protocol addresses, not only identity labels.

The protocol layer should not know what a profile, feed, post, avatar, or
community is. It should only let an identity publish signed bindings from
opaque paths to immutable content CIDs:

```text
{identity}.jolt/{path} -> ContentId
```

Application layers can later decide that `/profile`, `/feed`, `/apps/foo`, or
any other path has a specific document format.

## What to Build

Add the smallest user-facing path publishing flow on top of update logs.

Alice should be able to:

1. Publish bytes and receive a CID.
2. Bind an opaque path, such as `/hello`, to that CID in her signed update log.
3. Announce that her node can provide her update log.

Bob should be able to resolve Alice's signed path binding through the existing
resolver path.

## Acceptance Criteria

- [x] CLI publish can optionally bind the published CID to an opaque Jolt path.
- [x] HTTP publish can optionally bind the published CID to an opaque Jolt path.
- [x] Dashboard publish can optionally bind the published CID to an opaque Jolt path.
- [x] Publishing a path appends a signed `SetPath` update-log entry for the node identity.
- [x] The node announces itself as an update-log provider after publishing a path.
- [x] Local resolve returns the newly published path binding without copying update logs manually.
- [x] Network resolve can discover that path binding from another node through the existing update-log provider path.
- [x] Invalid paths are rejected before content is published.

## Notes

This card intentionally avoids profile/feed schemas. Those belong in later
application-layer cards once the generic signed namespace works.
