# 027: Relay Gossip v0

**Type:** HITL
**Milestone:** M5+
**Status:** Ready
**Blocked by:** None

## Why

The v0 discovery path should use Kademlia provider records first. Relay gossip is still likely needed later so relay operators can cooperate and share useful discovery hints without requiring every lookup to depend only on DHT behavior.

This card exists to make that future work explicit, not to pull it into the first bootstrap implementation.

## What to Decide

Before implementation, decide:

- What records relays gossip: update-log providers, content providers, relay reachability, or all of these.
- Whether gossip is push, pull, or periodic reconciliation.
- How relays bound memory and avoid spam.
- Whether relays gossip only signed/provider hints or also unsigned reachability observations.
- How clients treat gossip responses compared with DHT responses.

## What to Build

Define and implement a minimal relay-to-relay discovery-hint exchange.

The core rule remains:

```text
Relay gossip can locate candidates.
Relay gossip cannot define identity truth.
```

Bob must still verify signed update logs and content hashes locally.

## Acceptance Criteria

- [ ] Relay gossip protocol messages are documented.
- [ ] Relays can exchange update-log provider hints.
- [ ] Relays bound stored gossip hints.
- [ ] A client can query or benefit from gossiped hints when DHT-only discovery is insufficient.
- [ ] Invalid or malicious hints cannot make Bob accept unsigned state.
- [ ] Tests cover gossip exchange, stale hints, duplicate hints, and invalid candidate data.

## Non-Goals

- Content replication.
- Payments.
- Relay ranking marketplace.
- Global consensus.
