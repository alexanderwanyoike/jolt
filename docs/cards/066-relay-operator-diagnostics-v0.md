# 066: Relay Operator Diagnostics v0

**Type:** HITL  
**Milestone:** Relay Operations  
**Status:** Designed in PR
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

- [x] The design distinguishes local desktop Console diagnostics from headless
      relay operator diagnostics.
- [x] The design specifies the first CLI/admin API surfaces for relay health.
- [x] The design lists structured events/counters needed to debug relay mesh
      and identity/provider discovery failures.
- [x] The design includes security constraints for admin-only diagnostics on
      internet-facing hosts.
- [x] The design avoids application concepts and stays protocol/operator
      focused.

## Notes

This is not a product dashboard card. Treat relays as server software first:
logs, CLI, health APIs, and metrics before any optional UI.

## Design

See [Relay Operator Diagnostics](../17-relay-operator-diagnostics.md).

Key decisions:

- Jolt Console remains the local desktop diagnostics surface.
- Server-facing relays are debugged through SSH-friendly CLI commands,
  admin-only HTTP APIs, structured logs, and lightweight counters.
- Admin diagnostics are localhost-only by default. Public unauthenticated
  operator endpoints are not acceptable.
- The first implementation slice should be `jolt relay status --json` plus
  `GET /admin/v1/relay/status`.
- The most important later troubleshooting slice is
  `jolt relay diagnose identity <identity>`, which traces update-log provider
  discovery and relay forwarding for one identity.

## Follow-Up Implementation Slices

1. Relay CLI/Admin Status v0.
2. Relay Diagnose Identity v0.
3. Relay Structured Logs v0.
4. Relay Metrics v0.

## Verification

Docs-only design. No code tests were run.
