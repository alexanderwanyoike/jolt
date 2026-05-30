# 030: Persistent Update Log Store

**Type:** AFK
**Milestone:** M5 / Relay Availability
**Status:** Done
**Blocked by:** None

## Why

The manual relay demo proved that a fresh Bob can fetch new relay-pinned content while Alice is offline. It also exposed a real v0 bug:

```text
Alice publishes path A.
Bob resolves Alice and caches Alice's update log at sequence 0.
Alice restarts.
Alice publishes path B.
Alice's in-memory update log starts again at sequence 0.
Existing Bob keeps the old sequence-0 log and cannot see path B.
Fresh Bob can see path B because it has no older cached log.
```

That means Jolt's signed mutable state is not durable enough yet. Content survives daemon restart because published content is persisted, but the identity update log does not.

If `.jolt` addresses are the user's durable address space, Alice's update log must survive restarts and continue sequence numbers from the last signed entry.

## What to Build

Persist each node's own update log to disk and load it on daemon startup.

When Alice publishes a new path after restart, the node should append to the existing signed log instead of creating a new genesis entry with sequence `0`.

The persisted update log should be treated as signed protocol state, not as a cache:

- It belongs to the local identity.
- It is loaded before publishing new path bindings.
- It is served to peers through the update-log request protocol.
- It is announced as an update-log provider after startup when present.

## Acceptance Criteria

- [x] Publishing a path writes the signed update-log entry to durable storage.
- [x] Daemon startup loads the local identity's persisted update log.
- [x] Publishing after restart appends the next sequence number instead of creating a new sequence-0 log.
- [x] Existing Bob can resolve Alice's new path after Alice restarts and publishes again.
- [x] Relay pinning after restart pins the full latest update-log state.
- [x] Tests cover publish -> restart -> publish -> existing peer resolves both old and new paths.
- [x] Tests cover corrupted or invalid persisted update-log state failing closed with a clear error.

## Notes

This is not the same as Bob's cached copies of other users' update logs. This card is about the owner's authoritative local update log.

Do not add conflict resolution, multi-device identity merge, or update-log compaction here. Those are later protocol design problems.
