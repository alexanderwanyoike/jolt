# 026: Bootstrap Observability

**Type:** AFK
**Milestone:** M5
**Status:** Blocked by 019
**Blocked by:** 019

## Why

Bootstrap and discovery failures are otherwise invisible. A node may have no peers because config is missing, relays are unreachable, DHT bootstrap is still running, or discovery returned no candidates.

The dashboard and status API should make that state visible.

## What to Build

Expose minimal bootstrap state through status/API and dashboard:

```text
disconnected
bootstrapping
connected
degraded
```

Show enough context to debug:

- configured bootstrap relays
- effective bootstrap relays
- connected bootstrap peers
- cached relay/peer count, once Card 025 exists
- last bootstrap error, if any

## Acceptance Criteria

- [ ] Node status includes bootstrap state.
- [ ] Node status includes configured and effective bootstrap relay counts.
- [ ] Node status includes connected bootstrap peer count.
- [ ] Node status can represent disconnected, bootstrapping, connected, and degraded states.
- [ ] Dashboard renders bootstrap state without requiring logs.
- [ ] Tests cover status mapping for the main bootstrap states.

## Non-Goals

- Dashboard bootstrap editing.
- Relay ranking.
- Provider discovery UX.
- Full network graph visualization.
