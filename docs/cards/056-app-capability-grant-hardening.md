# 056: App Capability Grant Hardening

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Implemented in PR  
**Blocked by:** 042

## Why

Card 042's app boundary direction is sound, but private Pastey will add
encryption and decryption authority. Before that authority exists, app grants
must be strict enough that Console cannot accidentally approve capabilities
broader than an app requested.

## What to Build

- Parse app capabilities into strict internal capability values before grant
  validation.
- Reject malformed path scopes such as `publish:/pastes*`.
- Allow approving a requested scope exactly or narrowing it.
- Reject approving capabilities outside the app's requested capability set.
- Keep app capability parsing in the daemon/app boundary layer, not protocol.
- Document that private encryption/decryption APIs must not be added to the
  legacy trusted `/api/v1/*` surface.

## Acceptance Criteria

- [x] Admin approval rejects capabilities the app did not request.
- [x] Admin approval accepts a narrower path scope than requested.
- [x] Admin approval rejects malformed wildcard path scopes.
- [x] Existing app-session lifecycle and capability tests still pass.
- [x] No protocol-layer code knows about app capabilities.

## Verification

- Red: `cargo test -p jolt-server --test api_integration test_admin_cannot_approve_capabilities_beyond_app_request -- --nocapture`
- Red: `cargo test -p jolt-server --test api_integration test_admin_cannot_approve_malformed_path_scope_capabilities -- --nocapture`
- Green:
  - `cargo test -p jolt-server --test api_integration test_admin_cannot_approve_capabilities_beyond_app_request -- --nocapture`
  - `cargo test -p jolt-server --test api_integration test_admin_cannot_approve_malformed_path_scope_capabilities -- --nocapture`
  - `cargo test -p jolt-server --test api_integration capabilities -- --nocapture`
  - `cargo test -p jolt-server --test api_integration test_admin_can_approve_narrower_path_scope_than_app_requested -- --nocapture`
