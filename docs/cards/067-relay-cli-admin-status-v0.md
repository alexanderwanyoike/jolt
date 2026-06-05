# 067: Relay CLI/Admin Status v0

**Type:** AFK  
**Milestone:** Relay Operations  
**Status:** Ready after 066
**Blocked by:** 066

## Why

Relay operators need a quick SSH-friendly way to answer whether a headless relay
is healthy. The first implementation slice from
[066](066-relay-operator-diagnostics-v0.md) should expose a stable relay status
contract before deeper diagnosis, logs, or metrics.

## What to Build

Add a relay-focused status surface:

```text
jolt relay status
jolt relay status --json
GET /admin/v1/relay/status
```

The status should be derived from existing daemon state where possible:

- relay mode enabled/disabled;
- peer ID and identity address;
- API bind/port if available;
- listen addresses;
- bootstrap state, configured/effective relay counts, connected bootstrap peer
  count, and last bootstrap error;
- connected peer counts;
- known relay count;
- relay record summary when relay mode is enabled;
- cache/pin summary;
- current home relay config if relevant.

## Acceptance Criteria

- [ ] `jolt relay status` renders a compact human-readable relay health summary.
- [ ] `jolt relay status --json` returns a stable JSON payload.
- [ ] `GET /admin/v1/relay/status` returns the same relay status payload.
- [ ] The admin endpoint is local/admin-only and not exposed through app APIs.
- [ ] The command works over SSH against the local daemon.
- [ ] Tests cover CLI parsing, API response shape, relay-mode enabled state, and
      relay-mode disabled state.

## Notes

Do not build remote public admin auth in this card. For v0, remote relay
inspection means SSH into the host or tunnel to localhost.

Do not add app concepts. This card is about node/relay operation only.
