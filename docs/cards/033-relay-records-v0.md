# 033: Relay Records v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Ready  
**Blocked by:** 027

## Why

Before relays can gossip or help nodes explore the mesh, a relay needs a portable signed statement of who it is and how it can be reached.

A cold relay with no known relays is isolated. A cold relay with one valid relay record can start exploring. Relay records are the first primitive that lets this happen without making any relay an authority.

## What to Build

Define a signed relay record:

```text
RelayRecord {
  relay_id
  addrs
  capabilities
  observed_at
  expires_at
  signature
}
```

Capabilities should cover only current protocol behavior:

```text
bootstrap
discovery
pinning
```

The record is a claim by the relay about itself. Other nodes may store it, try it, and score it, but it is not proof that the relay is useful or honest.

## Acceptance Criteria

- [ ] Relay records serialize over the network wire format.
- [ ] Relay records are signed by the relay identity.
- [ ] Invalid signatures are rejected.
- [ ] Expired relay records are rejected.
- [ ] Relay capabilities are explicit and bounded.
- [ ] Status/API can expose the local relay's current record when relay mode is enabled.

## Verification

Automated deterministic tests are enough for this card.

No one-machine process demo is required yet because no exchange behavior exists.

No Hetzner canary is required.

## Non-Goals

- Relay-to-relay exchange.
- Relay scoring.
- Identity/provider gossip.
- Content replication.
