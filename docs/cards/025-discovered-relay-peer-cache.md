# 025: Discovered Relay and Peer Cache

**Type:** AFK
**Milestone:** M5
**Status:** Done
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

Cached peer addresses are only opportunistic reachability hints. They do not
grant trust, authority, content ownership, or permission to bypass signed Jolt
records. Configured bootstrap relays remain the stable fallback and source of
operator intent.

## Acceptance Criteria

- [x] Node persists discovered useful relay/node addresses.
- [x] Node uses cached addresses on later starts in addition to configured bootstrap relays.
- [x] Configured relays remain the fallback when cache is empty or stale.
- [x] Cache is bounded to a reasonable maximum size.
- [x] Cache entries can be expired or ignored when repeatedly unreachable.
- [x] Tests cover cache write, reload, fallback, and duplicate handling.
- [x] Docs explain cached peers are hints, not trusted authorities.

## Non-Goals

- Relay ranking marketplace.
- Reputation.
- Payment.
- Relay gossip.
