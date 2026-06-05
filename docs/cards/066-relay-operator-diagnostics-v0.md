# 066: Relay Operator Diagnostics v0

**Type:** HITL  
**Milestone:** Relay Operations  
**Status:** Ready for design
**Blocked by:** 063, 065

## Why

Retiring the daemon-served dashboard is the right product direction for local
users, but server-facing relays still need a serious operator debugging story.
A relay running on a VPS or home server should not depend on a desktop Console
or browser dashboard to explain whether it is healthy.

Relay operators need fast answers to questions like:

- Is this relay reachable from the mesh?
- Which peers and relays is it connected to?
- Is it forwarding identity/provider queries?
- Is it accepting and serving pins?
- Why can this relay not find a provider for identity `X`?
- Are bootstrap, gossip, and relay-record exchange working?

## What to Design

Define the v0 operator surface for headless relays:

- CLI diagnostics commands, for example `jolt status`, `jolt relay status`, or
  `jolt relay diagnose`.
- Admin-only relay diagnostics endpoints, with clear binding/auth expectations.
- Structured log fields/events for relay operations.
- Minimal counters or metrics that can later become Prometheus-compatible.
- A runbook-level troubleshooting flow for common relay failures.

## Acceptance Criteria

- [ ] The design distinguishes local desktop Console diagnostics from headless
      relay operator diagnostics.
- [ ] The design specifies the first CLI/admin API surfaces for relay health.
- [ ] The design lists structured events/counters needed to debug relay mesh
      and identity/provider discovery failures.
- [ ] The design includes security constraints for admin-only diagnostics on
      internet-facing hosts.
- [ ] The design avoids application concepts and stays protocol/operator
      focused.

## Notes

This is not a product dashboard card. Treat relays as server software first:
logs, CLI, health APIs, and metrics before any optional UI.
