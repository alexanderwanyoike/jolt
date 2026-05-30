# 041: Identity Head Gossip Fair Batching

**Type:** AFK
**Milestone:** M5+
**Status:** Done
**Blocked by:** 038

## Why

Identity-head gossip is a hint layer for common `.jolt` lookups. It must stay bounded as the network grows.

Card 038 bounded storage and exchange sizes, but the first exchange selector compared `latest_sequence` globally. That is not the right model because sequence numbers are meaningful only within a single identity. A busy identity should not consume the whole outbound gossip batch and starve unrelated identities.

## What to Build

Make outbound identity-head gossip batches fair across identities:

```text
stored hints:
  alice: seq 1, seq 2, seq 3
  bob:   seq 1
  tim:   seq 7

exchange batch:
  alice seq 3
  bob seq 1
  tim seq 7
```

Rules:

- Select the newest valid hint per identity.
- Do not compare sequence numbers across identities.
- Keep the exchange batch bounded.
- Rotate across identities when there are more identities than the exchange limit.
- Respect a peer's requested `GetIdentityHeads { limit }`.

## Acceptance Criteria

- [x] An identity with many provider hints cannot consume the whole outbound batch.
- [x] The newest valid hint is selected within each identity.
- [x] Repeated small exchanges rotate across identities instead of repeatedly returning the same first identities.
- [x] Existing identity-head gossip and `.jolt` resolution behavior still pass.

## Verification

Automated:

```text
cargo test -p jolt-network identity_head_exchange_batch -- --nocapture
```

Full regression:

```text
./scripts/test-local.sh
```

## Non-Goals

- Full DHT-style keyspace routing.
- Per-peer gossip digests.
- Relay scoring.
- Content provider gossip.
