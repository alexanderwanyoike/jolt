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

Add relay-to-relay query forwarding for identity/update-log providers:

```text
FindIdentityProviders {
  identity_id
  limit
}

IdentityProviders {
  identity_id
  providers
}
```

Providers are candidates only. Tim still verifies Alice's signed update log.

Flow:

```text
Tim -> R2: resolve Alice
R2 local lookup: miss
R2 asks selected known relays
R1 responds: Alice update log available here
R2 returns candidate to Tim
Tim fetches and verifies Alice update log
```

## Acceptance Criteria

- [ ] Relay can ask neighbour relays for identity/update-log providers.
- [ ] Relay can return bounded provider candidates to the requesting node.
- [ ] Query forwarding has hop/fanout/deadline limits.
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

No Hetzner canary is required for this card.

## Non-Goals

- Periodic identity-head gossip.
- Content provider gossip.
- Relay ranking or marketplace behavior.
