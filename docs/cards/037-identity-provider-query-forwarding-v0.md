# 037: Identity Provider Query Forwarding v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Ready  
**Blocked by:** 036

## Why

The biggest user-facing failure is:

```text
Someone shares alice.jolt, but my node cannot reach it.
```

If Tim only knows R2, and Alice's home relay is R1, Tim should not need to manually discover R1. R2 should be able to ask other relays whether they know where Alice's update log can be found.

## What to Build

Add bounded recursive relay-to-relay query forwarding for identity/update-log providers:

```text
FindIdentityProviders {
  query_id
  identity_id
  limit
  ttl
  deadline
}

IdentityProviders {
  query_id
  identity_id
  providers
}
```

Providers are candidates only. Tim still verifies Alice's signed update log.

Forwarding is recursive, but not unbounded:

- Each query has a unique `query_id`.
- Each relay keeps a short-lived seen-query cache.
- Each hop decrements `ttl`.
- Each relay forwards to only a small fanout of selected relay neighbours.
- Each query has a deadline.
- Responses aggregate back toward the requester.
- Relays may return partial results before the deadline.

Flow:

```text
Tim -> R2: resolve Alice
R2 local lookup: miss
R2 asks selected known relays with ttl=2
R1 local lookup: miss
R1 asks selected known relays with ttl=1
R3 responds: Alice update log available here
R1 returns candidate to R2
R2 returns candidate to Tim
Tim fetches and verifies Alice update log
```

## Acceptance Criteria

- [ ] Relay can ask neighbour relays for identity/update-log providers.
- [ ] Relay can forward a miss to selected neighbours while `ttl` remains.
- [ ] Relay suppresses duplicate query forwarding by `query_id`.
- [ ] Relay can return bounded provider candidates to the requesting node.
- [ ] Query forwarding has hop/fanout/deadline limits.
- [ ] Query forwarding returns partial results rather than hanging indefinitely.
- [ ] Invalid candidate data cannot make a client accept unsigned state.
- [ ] DHT-first lookup still works.
- [ ] Forwarding is used as fallback when local/DHT lookup is insufficient.

## One-Machine Process Demo

Required for review.

Run locally:

```text
Alice home relay: R1
Bob home relay: R3
Tim connected only to R2
Relay mesh: R1 - R2 - R3
```

Expected:

```text
Tim resolves Alice through R2 -> R1
Tim resolves Bob through R2 -> R3
Tim fetches both without knowing R1 or R3 directly
```

Also run a recursive case:

```text
Alice home relay: R4
Tim connected only to R1
Relay mesh: R1 - R2 - R3 - R4
```

Expected:

```text
Tim resolves Alice through bounded forwarding across R1 -> R2 -> R3 -> R4
The same query does not loop back forever
The lookup fails clearly if ttl is too small
```

No Hetzner canary is required for this card.

## Non-Goals

- Periodic identity-head gossip.
- Content provider gossip.
- Relay ranking or marketplace behavior.
