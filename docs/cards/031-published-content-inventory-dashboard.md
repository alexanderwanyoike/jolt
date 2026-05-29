# 031: Published Content Inventory Dashboard

**Type:** AFK
**Milestone:** M5 / Relay Availability
**Status:** Ready
**Blocked by:** 029

## Why

The dashboard can publish content and pin individual items to a home relay, but it still treats published content as a recent action rather than durable local state.

That is no longer enough. A node owner needs to understand their space:

```text
/a/b points at CID_1 locally.
/a/b was pinned to relay R at sequence 3.
/a/b now points at CID_2 locally, but CID_2 is not relay-backed yet.
```

This is starting to look like a Git-shaped model:

- Jolt paths are owner-signed refs.
- CIDs are immutable content snapshots.
- The update log is the signed history of ref changes.
- Pinning is closer to pushing selected state to a remote availability provider.

The dashboard should make that model visible in human terms without requiring users to remember CIDs, inspect logs, or reason about DHT provider state.

## What to Build

Add a node-owned published content inventory view.

The inventory should list every locally published path and content item the node knows it owns. For each row, show:

- Path, when the content was published with a Jolt path.
- Current local CID.
- Latest local update-log sequence for that path.
- Whether the latest local state is pinned to the configured home relay.
- The relay it is pinned to, using a readable label plus shortened peer/address details.
- The pinned sequence/CID the relay is expected to serve.
- A clear stale state when local state is newer than relay-backed state.
- A pin or repin action for items that are not relay-backed or are stale.

Add the API needed for the dashboard instead of forcing the UI to reconstruct this from unrelated endpoints.

For v0, it is acceptable for pin status to mean "this node successfully submitted this content and update-log snapshot to the configured home relay." Full independent availability probing belongs to Card 011.

## Acceptance Criteria

- [ ] HTTP API exposes a published inventory endpoint with path, local CID, local sequence, relay pin state, relay target, and pinned CID/sequence where known.
- [ ] Dashboard shows all locally published content, not just the most recent publish result.
- [ ] Dashboard clearly distinguishes local-only, relay-backed, and stale-local-newer-than-relay states.
- [ ] Dashboard gives a pin/repin action from the inventory list.
- [ ] Updating an already pinned path without repinning shows the item as stale.
- [ ] Repinning the updated path moves it back to relay-backed.
- [ ] A local manual demo can show Alice updating `/a/b`, Bob seeing latest while Alice is online, then Bob falling back to the pinned version after Alice goes offline.
- [ ] Tests cover inventory state mapping without testing dashboard scaffolding.

## Non-Goals

- Multi-relay pinning policy.
- Automatic pinning on every publish.
- Background availability repair.
- Relay billing, reputation, or storage-market behavior.
- Full Git semantics such as branches, merges, or diffs.

## Notes

Use product language in the dashboard:

- "Published locally" for content only Alice's node currently serves.
- "Pinned to relay" for content expected to survive Alice going offline.
- "Needs repin" when Alice changed a path after the last successful relay pin.

The user should not have to decide based on raw CID length or peer IDs. Those details can remain visible as secondary debug information.
