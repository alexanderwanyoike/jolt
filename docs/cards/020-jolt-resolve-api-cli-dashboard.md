# 020: Jolt Resolve API, CLI, and Dashboard

**Type:** AFK
**Milestone:** Human addressing / M5
**Status:** Blocked by 019
**Blocked by:** 019

## Why

The resolver exists as a network-layer primitive, but users cannot use it yet.

Today the dashboard connect box accepts peer multiaddrs, and the CLI fetch path accepts raw CIDs. That keeps Jolt in debug-tool mode. Bob should be able to enter a space or content address:

```text
{alice_identity}.jolt/feed
```

and see what the node resolved, which signatures were accepted, and which content target was selected.

## What to Build

Add a user-facing resolve flow:

```text
dweb resolve {identity}.jolt/feed
```

and HTTP/dashboard equivalents.

The flow should:

1. Parse the `.jolt` address.
2. Check the verified update-log cache.
3. Discover update-log providers through the bootstrap/relay mesh.
4. Request candidate update logs.
5. Verify signatures and sequence continuity.
6. Select the newest valid state.
7. Resolve the requested path to a `ContentId` and reachability hints.
8. Return a human-readable result and machine-readable JSON.

## Acceptance Criteria

- [ ] CLI has `dweb resolve <jolt-address>`.
- [ ] HTTP API exposes `POST /api/v1/resolve`.
- [ ] Dashboard has a `.jolt` resolve input separate from raw peer connect.
- [ ] Resolve output shows identity, path, selected sequence, content ID, and reachability hints.
- [ ] Invalid `.jolt` addresses fail with clear errors.
- [ ] Unresolved addresses explain whether bootstrap, discovery, or verification failed.
- [ ] Tests cover successful resolve, malformed address, no candidates, stale candidates, and invalid signatures.

## Non-Goals

- Fetching the content bytes. That belongs in Card 021.
- Petnames. That belongs in Card 015 after this works.
- Global usernames.
