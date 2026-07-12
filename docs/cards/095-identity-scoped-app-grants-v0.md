# 095: Identity-Scoped App Grants v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Done  
**Blocked by:** 092, 093

## Why

Apps are scoped to identities, not just to the local daemon. Once a user can have
multiple local identities and devices, the app section in Console must be
identity-aware.

An app grant should answer:

```text
Which app?
On which local device?
For which user identity?
With which capabilities?
```

## What to Build

Update app session and Console grant handling so app authority is explicitly
identity-scoped:

- app approval requests name the requested user identity;
- grants are listed under the selected identity in Console;
- app APIs require the session's identity scope;
- revoking an app grant affects that app for that identity only;
- device revocation invalidates or blocks app sessions bound to that device;
- diagnostics make identity/device/app grant boundaries visible.

## Acceptance Criteria

- [x] The same app can be approved for one local identity without being approved
      for another.
- [x] Console shows pending, active, rejected, and revoked grants for the
      selected identity.
- [x] App APIs cannot silently operate on the wrong local identity.
- [x] Revoking an app grant for identity A does not revoke identity B's grant.
- [x] Revoking a device prevents that device's app sessions from continuing to
      write as the user identity.
- [x] Tests cover cross-identity grant isolation.

## Non-Goals

- App store/catalog work.
- Browser origin permissions.
- App-specific UI inside Console.

## Notes

Keep the daemon/app boundary generic. Console may display app names and
capabilities, but protocol code must not hardcode Pastey or Spoke concepts.

## Implementation Notes

- App session requests without an explicit requested identity are now bound to
  the Console-selected local identity at request time, so pending approval rows
  always name the user identity being requested.
- Admin app request/session listings are scoped to the selected local identity:
  pending and rejected requests are returned from `/admin/v1/app-requests`, and
  active/revoked sessions are returned from `/admin/v1/app-sessions`.
- Admin app session revocation is identity-scoped. Revoking a session while
  identity A is selected cannot revoke the same app's session for identity B.
- Write/private local-authority grants (`publish:*`, `inventory:*`,
  `pin:own:*`, `encrypt:*`, `decrypt:*`) now require the daemon-signable local
  identity at approval time. Unknown or generated non-signing identities can no
  longer be approved for those capabilities and then fail later at API use.
- App session records and views now carry a transitional `device_id`, currently
  `dev_legacy_root`, matching the card-093/094 local writer path. Revoking a
  device revokes active app sessions bound to that device, so their bearer tokens
  stop authenticating.
- Console's Apps section now renders selected-identity request history rather
  than only pending requests, keeping rejected requests visible and disabling
  approve/reject actions for non-pending rows.

## Verification

- Red first:
  - `cargo test -p jolt-server test_admin_app_grants_are_scoped_to_selected_local_identity --test api_integration -- --nocapture`
    failed because `/admin/v1/app-sessions` listed grants globally.
  - `cargo test -p jolt-server test_admin_app_requests_are_scoped_to_selected_local_identity --test api_integration -- --nocapture`
    failed because identity-less requests were stored as `null` and disappeared
    from selected-identity request lists.
  - `cargo test -p jolt-server test_admin_cannot_grant_publish_capability_to_unknown_identity --test api_integration -- --nocapture`
    failed because unknown identities could be approved for `publish:*`.
  - `cargo test -p jolt-server test_revoking_local_device_revokes_its_app_sessions --test api_integration -- --nocapture`
    failed because app sessions had no `device_id`.
  - `npm test` from `apps/jolt-console` failed because rejected app requests
    were filtered out of the Apps section.
- Green:
  - `cargo test -p jolt-server app_ --test api_integration -- --nocapture`
  - `cargo test -p jolt-server device_authority --test api_integration -- --nocapture`
  - `npm test` from `apps/jolt-console`
- Full local suite:
  - `./scripts/test-local.sh` still fails in the pre-existing
    `two_nodes_dht_provider_announce_and_fetch` DHT integration case; the fetch
    result was empty bytes instead of the expected content. This is the same
    network-dependent flake family already recorded on card 094.
