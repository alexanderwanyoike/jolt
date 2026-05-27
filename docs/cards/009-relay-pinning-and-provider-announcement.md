# 007: Relay Pinning and Provider Announcement

**Type:** AFK  
**Milestone:** M4.5  
**Status:** Blocked  
**Blocked by:** 008

## Why

A home relay needs to keep owner-requested content reachable while the owner's device is offline.

## What to Build

Teach a relay-capable node to accept verified pin requests, store the content, mark it pinned, and announce itself as a provider for the pinned content.

End-to-end behavior:

```text
Alice node sends signed pin request + content to relay.
Relay verifies request.
Relay stores and pins content.
Relay announces provider record.
Other nodes can fetch the content from the relay.
```

## Acceptance Criteria

- [ ] Relay accepts a valid signed pin request.
- [ ] Relay rejects invalid pin requests.
- [ ] Accepted content is stored as pinned and survives cache eviction.
- [ ] Relay announces provider records for pinned content.
- [ ] Another node can fetch pinned content from the relay while the publisher is disconnected in a test.

## Notes

Use existing content store pinning where possible. Keep relay capability explicit so ordinary nodes are not forced to accept third-party pins.
