# 011: Availability Checks v0

**Type:** AFK  
**Milestone:** M4.5  
**Status:** Done
**Blocked by:** None

## Why

Bob should not have to think about whether his relay is still serving his content. Bob's node should check that.

## What to Build

Add basic node-managed availability checks for home-relay-pinned content.

For v0, checking can be simple:

- Periodically ask the configured home relay for pinned content status.
- Optionally perform a fetch/verify probe for important pinned CIDs.
- Surface degraded availability in status/API output.

Do not implement automatic multi-relay repair yet unless it falls out naturally.

## Acceptance Criteria

- [x] Node tracks which local published CIDs are expected to be pinned on home relay.
- [x] Node can ask relay whether a CID is pinned.
- [x] Node status/API reports healthy vs degraded relay availability.
- [x] A failed relay check does not break local publishing/fetching.
- [x] Tests cover healthy relay, missing pin, and unreachable relay.

## Result

Implemented as an on-demand v0 check:

- Relays expose `GET /api/v1/relay/pins/{content_id}` for pinned-CID status.
- Owner nodes expose `GET /api/v1/home-relay/availability` for recorded home-relay pins.
- The dashboard can manually check home relay availability from the Relay panel.

This intentionally avoids automatic repair or multi-relay policy.

## Notes

This is about transparency without burdening the user. Avoid complex policy language.
