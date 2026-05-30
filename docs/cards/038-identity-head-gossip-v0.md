# 038: Identity Head Gossip v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Ready  
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

- [ ] Relays can exchange identity-head hints.
- [ ] Hints are validated before storage.
- [ ] Hints expire.
- [ ] Hints are bounded globally and per identity.
- [ ] Duplicate/stale hints are ignored.
- [ ] Tim can resolve an identity using a fresh gossiped hint.
- [ ] Malicious hints cannot make Tim accept unsigned state.

## One-Machine Process Demo

Required for review.

Run locally:

```text
Alice home relay: R1
Tim connected only to R2
R2 has a fresh gossip hint for Alice from R1
```

Expected:

```text
Tim resolves Alice through R2 without live query forwarding.
Tim still verifies Alice's signed update log.
```

No Hetzner canary is required for this card.

## Non-Goals

- CID provider gossip for every content item.
- Pubsub event streams.
- Global consensus.
