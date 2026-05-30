# 027: Relay Gossip v0

**Type:** HITL
**Milestone:** M5+
**Status:** Split into 033-039
**Blocked by:** None

## Why

Relay gossip is not one feature. It splits into several smaller protocol jobs:

```text
Relay discovery:
  What relays exist? How do new nodes and relays learn about them?

Identity/provider discovery:
  Which relay might know where alice.jolt's signed update log is?

Hint dissemination:
  Which signed, expiring hints are worth sharing before a lookup asks for them?
```

The important failure case is:

```text
Someone shares alice.jolt, but my node cannot reach it.
```

A cold relay with no known relays is still isolated. A relay with one known relay should be able to ask around. Nodes should also learn relays from connected relays and announce verified relay records they already know.

This card is now the umbrella design card. Implementation has been split into smaller cards so each slice can be verified locally without requiring a full internet canary.

## Design Line

Relay gossip is a hint layer:

```text
Gossip tells you where to look.
Signatures and CIDs tell you what is true.
```

Relays may lie, be stale, or be unreachable. Clients must still verify identity signatures, update logs, and content hashes.

## Split Cards

- [033: Relay Records v0](033-relay-records-v0.md)
- [034: Relay Address Book v0](034-relay-address-book-v0.md)
- [035: Relay Record Exchange v0](035-relay-record-exchange-v0.md)
- [036: Relay Mesh Exploration v0](036-relay-mesh-exploration-v0.md)
- [037: Identity Provider Query Forwarding v0](037-identity-provider-query-forwarding-v0.md)
- [038: Identity Head Gossip v0](038-identity-head-gossip-v0.md)
- [039: Relay Discovery Failure UX](039-relay-discovery-failure-ux.md)

## Verification Strategy

Most relay gossip cards should be verified with:

```text
1. Automated deterministic tests.
2. One-machine multi-process demos where network behaviour matters.
3. One real canary only at the relay-mesh milestone boundary.
```

The one-machine demos are important because the reviewer only needs local process communication to see whether relay exchange, exploration, forwarding, and failure states are real.

The internet canary should only prove the full milestone once the slices are already working locally.

## Non-Goals

- Content replication.
- Payments.
- Relay ranking marketplace.
- Global consensus.
- Full pubsub event streams.
