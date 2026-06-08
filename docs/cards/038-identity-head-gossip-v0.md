# 038: Identity Head Gossip v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Done
**Blocked by:** 037

## Why

Query forwarding lets a relay ask around when it does not know an identity. Identity-head gossip makes common lookups faster by letting relays exchange small, bounded hints ahead of time.

Gossip must spread where to look, not what to believe.

## What to Build

Relays exchange bounded identity-head hints:

```text
IdentityHeadHint {
  identity_id
  provider
  relay_hint
  latest_sequence
  update_log_head
  observed_at
  expires_at
  owner_signature
}
```

Rules:

- Owner signature is required.
- Relay observations are not authority.
- Newer valid sequences replace older valid hints.
- Hints expire.
- Hints are bounded per identity and per peer.
- Clients still fetch and verify the signed update log.

## Acceptance Criteria

- [x] Relays can exchange identity-head hints.
- [x] Hints are validated before storage.
- [x] Hints expire.
- [x] Hints are bounded globally and per identity.
- [x] Duplicate/stale hints are ignored.
- [x] Tim can resolve an identity using a fresh gossiped hint.
- [x] Malicious hints cannot make Tim accept unsigned state.

## Implemented

Jolt now has an owner-signed `IdentityHeadHint`:

```text
IdentityHeadHint {
  owner_public_key
  identity
  provider_peer_id
  provider_addrs
  relay_hint
  latest_sequence
  update_log_head
  observed_at
  expires_at
  signature
}
```

The signature covers the hint body. The identity must match the owner public key. Relays reject expired, stale, duplicate, and tampered hints before storage.

Relay gossip exchanges these hints during relay-to-relay exchange:

```text
AnnounceIdentityHeads { hints }
GetIdentityHeads { limit }
IdentityHeads { hints }
```

The common path is now:

```text
Alice/R1 publishes signed update-log state
R1 creates an owner-signed identity-head hint
R2 learns the hint through relay gossip
Tim asks R2 who can provide Alice's update log
R2 returns the gossiped provider hint immediately
Tim fetches Alice's update log from the hinted provider
Tim verifies Alice's signed update log before accepting the path
```

Query forwarding from card 037 remains the fallback path when a relay has no fresh hint.

## One-Machine Process Demo

Required for review.

Run locally:

```text
Alice/R1 relay publishes content
Tim connected only to R2
R2 has a fresh gossip hint for Alice from R1
```

Expected:

```text
Tim resolves Alice through R2 without live query forwarding.
Tim still verifies Alice's signed update log.
```

No Hetzner canary is required for this card.

Implemented as:

```text
./scripts/test-identity-head-gossip-process.sh
```

## Non-Goals

- CID provider gossip for every content item.
- Pubsub event streams.
- Global consensus.
