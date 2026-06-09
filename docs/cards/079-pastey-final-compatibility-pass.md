# 079: Pastey Final Compatibility Pass

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** In PR
**Blocked by:** 075, 076

## Why

Pastey was the first app-boundary proof. Before v0 freezes, it should be
checked against the final Jolt APIs so it remains a credible companion app and
regression canary.

## What to Build

In the Pastey repo:

- remove dev-only assumptions where practical;
- verify app-session request/approval still works;
- verify public paste publish/fetch still works;
- verify private/self-only paste still works;
- verify recipient private paste still works;
- verify optional pinning behavior;
- update setup docs to point at packaged Jolt if available.

## Acceptance Criteria

- [x] Pastey works against current Jolt `dev`.
- [x] Pastey does not use admin APIs for normal app behavior.
- [x] Public paste workflow passes.
- [ ] Private paste workflow passes.
- [ ] Optional pinning behavior is documented.
- [ ] README/setup instructions are current.

## Non-Goals

- Turning Pastey into the flagship product.
- Adding social features to Pastey.
- App store integration.

## Notes

Pastey remains useful as a technical PoC even if Spoke becomes the human-facing
PoC.

Jolt PR: `https://github.com/alexanderwanyoike/jolt/pull/123`.
Pastey PR: `https://github.com/alexanderwanyoike/pastey/pull/4`.

Compatibility pass found and fixed:

- Pastey now uses the current `/app/v1/*` and `/api/v1/*` daemon endpoints.
- Pastey can request approval without knowing the local identity up front.
- Jolt can approve local-identity app-session requests without requiring the app
  to submit the daemon identity address.
- Pastey keeps session refresh working even when best-effort daemon status is
  unavailable.
- Jolt daemon startup no longer treats cached discovered peer hints as bootstrap
  relays. Those hints are opportunistic, often ephemeral local test daemon
  addresses, and caused default Iroh startup/command responsiveness problems
  after multi-node local testing.
- Jolt daemon command handles now time out instead of letting HTTP/app callers
  wait forever if the node loop stops answering.

Verification:

- Green: `cargo test -p jolt-server test_admin_can_approve_app_session_request_for_local_identity --test api_integration`.
- Green: `cargo test -p jolt-network daemon_handle_can_expose_startup_local_identity_address`.
- Green: `cargo test -p jolt-network daemon_command_status_reports_basic_node_state`.
- Green: `cargo test -p jolt-network daemon_response_times_out`.
- Green: `cargo test -p jolt-node build_network_config_adds_learned_relays_but_ignores_cached_peer_hints`.
- Green: `./scripts/test-local.sh`.
- Green: `npm test`, `npm run build`, `cargo test --manifest-path src-tauri/Cargo.toml`, and `npm run desktop:build` in Pastey.
- Manual: packaged Jolt Console + Pastey AppImage can request approval, approve
  in Console, publish a public paste, and list published content.
- Manual daemon regression: copied the real Jolt profile containing 28 cached
  discovered peer hints, started the default Iroh daemon on isolated port
  `9976`, and verified `60/60` `/api/v1/status` probes succeeded while logs
  reported `Effective bootstrap relays: 0`.
