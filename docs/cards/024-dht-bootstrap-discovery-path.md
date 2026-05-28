# 024: DHT Bootstrap Discovery Path

**Type:** AFK
**Milestone:** M5
**Status:** Done
**Blocked by:** 019

## Why

Card 019 gives a fresh node bootstrap relay configuration. This card proves the configured relay actually helps Bob discover Alice's signed state.

The v0 decision is DHT first, relay gossip later:

```text
Bob -> configured bootstrap relay -> DHT routing table -> providers for jolt:update-log:{identity}
```

Relays do not become authorities. They help Bob find candidates. Bob still verifies Alice's signed update log.

## What to Build

Implement and test the first DHT-backed global discovery path:

```text
Alice
  -> can serve Alice's update log
  -> announces provider for jolt:update-log:{alice_identity}

Relay
  -> acts as Bob's configured bootstrap peer
  -> participates in DHT routing/provider discovery

Bob
  -> starts fresh with only relay config
  -> bootstraps through relay
  -> discovers Alice as provider for jolt:update-log:{alice_identity}
  -> requests and verifies Alice's update log
```

## Acceptance Criteria

- [x] A node can announce itself as an update-log provider under `jolt:update-log:{identity}`.
- [x] A fresh node can bootstrap through a configured relay.
- [x] Bob can discover an update-log provider through the DHT after bootstrapping through the relay.
- [x] Bob can request the candidate update log after provider discovery.
- [x] Bob verifies the update log before storing it.
- [x] Test covers Alice -> Relay -> Bob with Bob starting from no Alice state.
- [x] Invalid candidate logs are rejected in the discovery path.

## Non-Goals

- Relay-to-relay gossip.
- Discovered peer cache.
- Dashboard bootstrap state.
- Fetching content by `.jolt` address. That belongs in Card 021.
- Alice-offline relay availability. That belongs in Card 022 after relay pinning exists.
