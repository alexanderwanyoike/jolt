# 019: Bootstrap Config and Relay Mode v0

**Type:** AFK
**Milestone:** M5
**Status:** Ready
**Blocked by:** None

## Why

`.jolt` addresses are only useful after a node can find the Jolt network.

A fresh node cannot bootstrap from a `.jolt` address because resolving `.jolt` already requires access to discovery. The node needs at least one raw network address first:

```text
/dns4/bootstrap-1.jolt.network/udp/4001/quic-v1/p2p/12D3...
/ip4/203.0.113.10/udp/4001/quic-v1/p2p/12D3...
```

Those bootstrap relays are not authorities over user identity. They are entry points into discovery. Signed update logs remain the source of truth.

This card creates the configuration and mode surface for that entry path. It does not need to prove full provider discovery yet.

## Decisions

Use the following v0 decisions:

- Bootstrap sources are config plus optional built-in defaults.
- No hardcoded public production relays yet.
- A node may be marked as a bootstrap/discovery relay, but ordinary nodes can still be dialed explicitly.
- DHT provider records are the first discovery mechanism. Relay gossip is a later card.
- Bootstrap management CLI is a later card. This card can expose config and status without adding `dweb bootstrap add/remove`.

## What to Build

Add the smallest persistent bootstrap configuration surface:

- Node config can store bootstrap relay multiaddrs.
- Node startup combines configured bootstrap relays with optional built-in defaults.
- Node status/API can report configured and effective bootstrap relays.
- Node config can mark the node as acting as a bootstrap/discovery relay.
- Relay mode is capability/intent metadata for v0; it should not imply pinning or storage-market behavior.

The first implementation can keep current `--bootstrap` behavior working. Persistent config should make it possible for a fresh node to start without retyping relay addresses every time.

## Acceptance Criteria

- [ ] Node config has explicit bootstrap relay multiaddrs.
- [ ] Startup uses configured bootstrap relay multiaddrs.
- [ ] Startup can also include optional built-in defaults when configured relays are absent.
- [ ] There are no hardcoded public production relay addresses.
- [ ] Config can mark a node as a bootstrap/discovery relay.
- [ ] Status/API reports configured bootstrap relays and effective bootstrap relays.
- [ ] Existing `--bootstrap` startup still works.
- [ ] Tests cover config load/save, effective relay selection, and relay-mode status.
- [ ] Docs explain bootstrap multiaddrs are entry points, while `.jolt` addresses are identity/content addresses.

## Non-Goals

- `dweb bootstrap add/remove/list`. That belongs in Card 023.
- Proving DHT provider discovery through a relay. That belongs in Card 024.
- Discovered relay/peer cache. That belongs in Card 025.
- Dashboard bootstrap state. That belongs in Card 026.
- Relay-to-relay gossip. That belongs in Card 027.
- Payment, relay ranking, or storage-market behavior.
