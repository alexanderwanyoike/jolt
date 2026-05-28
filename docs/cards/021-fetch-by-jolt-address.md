# 021: Fetch by Jolt Address

**Type:** AFK
**Milestone:** Human addressing / M5
**Status:** Ready
**Blocked by:** None

## Why

Resolving a `.jolt` address is not enough. The user wants the content.

Bob should not need to copy a resolved CID into a second command. The fetch path should accept both immutable CIDs and mutable `.jolt` addresses:

```text
dweb fetch bafk...
dweb fetch {alice_identity}.jolt/feed
```

## What to Build

Extend fetch so a `.jolt` address becomes:

```text
.jolt address
  -> verified resolve
  -> ContentId + reachability hints
  -> provider discovery
  -> content request
  -> cache and return bytes
```

The dashboard fetch box should accept the same values.

## Acceptance Criteria

- [ ] CLI `dweb fetch <target>` accepts either a CID or `.jolt` address.
- [ ] HTTP fetch API accepts either a CID or `.jolt` address.
- [ ] Dashboard fetch input accepts either a CID or `.jolt` address.
- [ ] Fetch-by-address uses the same signature verification path as `dweb resolve`.
- [ ] Successful fetch caches the content locally.
- [ ] Errors distinguish resolve failure from content-provider/fetch failure.
- [ ] Tests cover CID fetch, `.jolt` fetch, unresolved `.jolt`, and resolved-but-unavailable content.

## Non-Goals

- Local aliases/petnames.
- Space/feed UI beyond fetching the resolved bytes.
- Content encryption.
