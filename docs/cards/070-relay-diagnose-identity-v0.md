# 070: Relay Diagnose Identity v0

**Type:** AFK  
**Milestone:** Relay Operations  
**Status:** Implemented in PR
**Blocked by:** 066, 067

## Why

Relay operators need a fast SSH-friendly answer to why a relay cannot find an
identity's update-log provider. The old dashboard is gone, and server-facing
relays should be debugged through CLI/admin contracts rather than a desktop
Console.

## What to Build

Add an identity-provider diagnosis surface:

```text
jolt relay diagnose identity <identity>
jolt relay diagnose identity <identity> --json
POST /admin/v1/relay/diagnose/identity
```

The diagnosis should stay protocol/operator focused:

- target identity and deterministic update-log provider key;
- local verified update-log cache hit/miss;
- local identity-head hint hit/miss;
- local provider candidates;
- relay forwarding attempts and per-relay response/failure/timeout status;
- final structured outcome using existing discovery failure vocabulary where
  possible.

## Acceptance Criteria

- [x] `POST /admin/v1/relay/diagnose/identity` returns a structured diagnosis.
- [x] `jolt relay diagnose identity <identity>` renders compact human output.
- [x] `jolt relay diagnose identity <identity> --json` returns the same JSON
      payload as the admin endpoint.
- [x] Diagnosis reports local update-log cache hit/miss.
- [x] Diagnosis reports local identity-head hint hit/miss.
- [x] Diagnosis reports local provider candidates when known.
- [x] Diagnosis sends relay provider queries when connected relay targets are
      available and reports per-relay attempt status.
- [x] Failure outcomes reuse existing discovery codes such as
      `no_bootstrap_relays` and `identity_provider_not_found`.
- [x] The endpoint is not exposed through app APIs.
- [x] No app concepts such as inboxes, messages, contacts, feeds, or Pastey are
      introduced.

## Notes

This is still v0. It does not add remote admin authentication, public operator
endpoints, metrics, or structured logs. Remote use remains SSH into the relay
host or tunnel to the local admin API.

## Implementation Notes

- Added a daemon `DiagnoseIdentity` command and public response DTOs in
  `jolt-network`.
- Added `POST /admin/v1/relay/diagnose/identity`.
- Added `jolt relay diagnose identity <identity> [--json]`.
- The daemon diagnosis reuses existing update-log provider keys,
  identity-head hints, relay exchange `FindIdentityProviders`, and discovery
  failure codes.
- Per-relay attempts return `responded`, `failed`, or `timeout` status.

## Verification

- Red: `cargo test -p jolt-server test_admin_relay_diagnose_identity_reports_no_bootstrap_relays --test api_integration -- --nocapture`
  failed with 404 before the admin route existed.
- Red: `cargo test -p jolt-node parse_relay_diagnose_identity_command -- --nocapture`
  failed before the new CLI subcommand was handled.
- Green: `cargo test -p jolt-server relay_diagnose --test api_integration -- --nocapture`.
- Green: `cargo test -p jolt-node relay -- --nocapture`.
- Green: `cargo fmt --check`.
- Green: `cargo check -p jolt-network -p jolt-server -p jolt-node`.
- Green: `./scripts/test-local.sh`.
