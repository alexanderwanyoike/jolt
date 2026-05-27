# 019: Bootstrap Relay Mesh v0

**Type:** HITL
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

## Product Constraint

Do not turn this into a storage market.

The v0 mesh should answer one question:

```text
Can Bob's fresh node join the network and find candidates for Alice's signed state?
```

It should not introduce payments, relay ranking, content replication markets, or automatic relay-to-relay content copying.

## What to Decide

Before implementation, decide:

- What bootstrap multiaddrs ship in local config during development.
- Whether default bootstrap addresses live in code, config, or both.
- Whether relays gossip provider/update-log knowledge directly or rely only on Kademlia provider records for v0.
- How many relays Bob should query before declaring a `.jolt` address unresolved.
- What local cache is kept after successful bootstrap.
- What the dashboard should show when bootstrap/discovery is degraded.

## What to Build

Implement the smallest global entry path:

```text
fresh node
  -> configured bootstrap relay multiaddrs
  -> DHT bootstrap/routing table
  -> discover providers for jolt:update-log:{identity}
  -> request and verify update logs from candidates
```

The first relay mesh does not need to store content for other users. It only needs to participate in discovery and routing.

## Acceptance Criteria

- [ ] Node config has explicit bootstrap relay multiaddrs.
- [ ] CLI can list the effective bootstrap relays.
- [ ] CLI can add and remove user-configured bootstrap relays.
- [ ] A fresh node can join through a configured bootstrap relay.
- [ ] A relay participates in DHT/provider discovery for update-log provider keys.
- [ ] A node caches useful discovered relay/node addresses for future starts.
- [ ] Dashboard reports bootstrap state: disconnected, bootstrapping, connected, degraded.
- [ ] Tests cover fresh node -> bootstrap relay -> discover update-log provider.
- [ ] Docs explain bootstrap multiaddrs are entry points, while `.jolt` addresses are identity/content addresses.

## Non-Goals

- Global usernames.
- Payment.
- Relay ranking.
- Automatic owner-unapproved relay-to-relay content replication.
- Long-term public bootstrap governance.
