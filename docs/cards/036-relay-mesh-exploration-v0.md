# 036: Relay Mesh Exploration v0

**Type:** AFK  
**Milestone:** M5+  
**Status:** Ready  
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

- [ ] A relay can learn new relay records through a known relay.
- [ ] Exploration is rate-limited and bounded.
- [ ] Invalid/expired relay records are rejected.
- [ ] Learned relays persist.
- [ ] Status/API can expose learned relay count.

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
