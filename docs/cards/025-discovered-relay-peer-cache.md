# 025: Discovered Relay and Peer Cache

**Type:** AFK
**Milestone:** M5
**Status:** Ready
**Blocked by:** None

## Why

A fresh node needs configured bootstrap relays to enter the mesh. After it has successfully joined, it should remember useful discovered relay/node addresses so it is less dependent on the original bootstrap set next time.

This follows the BitTorrent-style pattern:

```text
configured bootstrap contacts
  -> join the network
  -> learn more useful peers
  -> cache them for future starts
```

Configured bootstrap relays remain the fallback. Cached addresses are optimizations, not authorities.

## What to Build

Persist a small cache of useful discovered relay/node addresses.

The cache should prefer addresses that helped with:

- successful bootstrap
- DHT routing
- update-log provider discovery
- content provider discovery

The cache should be bounded and safe to discard.

## Acceptance Criteria

- [ ] Node persists discovered useful relay/node addresses.
- [ ] Node uses cached addresses on later starts in addition to configured bootstrap relays.
- [ ] Configured relays remain the fallback when cache is empty or stale.
- [ ] Cache is bounded to a reasonable maximum size.
- [ ] Cache entries can be expired or ignored when repeatedly unreachable.
- [ ] Tests cover cache write, reload, fallback, and duplicate handling.
- [ ] Docs explain cached peers are hints, not trusted authorities.

## Non-Goals

- Relay ranking marketplace.
- Reputation.
- Payment.
- Relay gossip.
