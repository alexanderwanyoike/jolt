# 094: Per-Device Writer Logs and Deterministic Merge v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 091 and 093  
**Blocked by:** 091, 093

## Why

True multi-writer identity cannot rely on one global identity log sequence
shared by every device. That recreates a single-writer bottleneck and makes
offline/device races brittle.

Each authorized device should be able to publish its own signed writer log. The
resolved user identity state is then a deterministic materialized view over
authorized, non-revoked device logs.

## What to Build

Implement the minimal true multi-writer identity state path:

- publish a per-device signed writer log;
- discover writer logs from authorized device records;
- verify log entries against device authorization state;
- merge multiple device logs deterministically;
- expose merged identity path state through existing resolve APIs;
- preserve losing/conflicting records for diagnostics;
- make same-identity multi-device reads deterministic.

## Acceptance Criteria

- [ ] Two authorized devices can publish independent identity state while both
      are online.
- [ ] `.jolt` resolution produces the same merged result regardless of device
      log discovery order.
- [ ] Concurrent append-style app records from different devices can coexist.
- [ ] Concurrent singleton path updates resolve deterministically.
- [ ] Conflict history is inspectable enough for diagnostics.
- [ ] Writes from revoked devices are ignored after revocation.
- [ ] Tests cover two-device concurrent publishes and deterministic merge.

## Non-Goals

- Generic CRDT support for arbitrary app payloads.
- Automatic merging of app-specific documents.
- Global total ordering across all Jolt identities.
- Protocol-level knowledge of profiles, posts, feeds, or pastes.

## Notes

Singleton paths such as `/profile` need deterministic winner selection.
Append-style app records should normally coexist. Apps remain responsible for
interpreting their own object schemas.

