# 004: Add Update Log Core

**Type:** AFK  
**Milestone:** M4  
**Status:** Done
**Blocked by:** None

## Why

Jolt currently fetches immutable content by CID. To become a user-owned web, it needs a way to ask:

```text
What is Alice's latest state?
```

That requires a signed append-only update log. The log is the bridge from immutable blobs to mutable user-owned web presence.

## What to Build

Add core update log types and verification:

- Log entry sequence numbers.
- Previous-entry hash chaining.
- Actions for publishing content, updating root content, and updating profile metadata.
- Owner signatures over canonical entry bytes.
- Verification that the chain is ordered, signed, and not tampered with.

This card only needs local data structures and tests. It does not need network sync yet.

## Acceptance Criteria

- [x] A user identity can create a genesis log entry.
- [x] A user identity can append a signed log entry.
- [x] Verification rejects entries signed by the wrong key.
- [x] Verification rejects broken previous-entry hashes.
- [x] Verification rejects out-of-order sequence numbers.
- [x] Tests cover publish content, update root, and profile update actions.

## Notes

Prefer putting shared primitives in `dweb-core` unless implementation pressure shows a better crate boundary.
