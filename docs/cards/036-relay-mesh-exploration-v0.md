# 036: Relay Mesh Exploration v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Done
**Blocked by:** 035

## Why

A cold relay is isolated unless it knows at least one relay. Once it knows one relay, it should be able to ask around and slowly discover more of the relay mesh.

This should not be global flooding. It should be bounded exploration.

## What to Build

When relay mode is enabled, a relay periodically asks a small number of known relays for more relay records:

```text
R2 knows R1
R2 asks R1 for relays
R1 returns R3, R4
R2 stores verified records
R2 may later ask R3 or R4
```

Rules:

- Use signed relay records only.
- Bound fanout per interval.
- Bound records accepted per peer.
- Do not ask every known relay every interval.
- Prefer relays that have responded successfully.
- Expire stale records.

## Acceptance Criteria

- [x] A relay can learn new relay records through a known relay.
- [x] Exploration is rate-limited and bounded.
- [x] Invalid/expired relay records are rejected.
- [x] Learned relays persist.
- [x] Status/API can expose learned relay count.

## Implementation Notes

- Relay-mode nodes periodically explore the relay mesh.
- Each exploration tick selects at most one known relay record, so a relay does not ask every known relay every interval.
- Exploration uses the existing signed relay exchange protocol from card 035.
- Relay exchange responses mark the responding relay as successful, so later address-book ordering can prefer useful relays.
- Learned relays are persisted in the local relay address book and are visible through the existing known relay count in status/API/dashboard.

## One-Machine Process Demo

Required for review.

Run locally:

```text
R1 knows R2
R2 knows R3
R3 knows nobody else
```

Expected:

```text
R1 eventually learns R3 through R2
R3 eventually learns R1 through R2
```

Verifier should be able to inspect known relay records on each process.

No Hetzner canary is required.

## Non-Goals

- Identity lookup forwarding.
- Identity-head gossip.
- Content provider gossip.
