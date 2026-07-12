# 092: Multiple Local Identities v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Done
**Blocked by:** 091

## Why

Console and the daemon need to manage more than one local user identity. Without
that, app grants, diagnostics, and app data all assume there is one local person
using one local namespace.

Multiple local identities are also the first visible step toward clean
multi-device identity management.

## What to Build

Add a first-class local identity selection model:

- list local identities;
- create an additional local identity;
- name additional local identities during creation;
- select the active identity for Console views;
- delete generated local identities without allowing the daemon signing identity
  to be removed;
- expose the selected identity in app approval flows;
- make identity-scoped state obvious in Console;
- keep daemon/admin APIs explicit about which identity they operate on.

## Acceptance Criteria

- [x] A local node can hold at least two user identities.
- [x] Console can switch between local identities without restarting the daemon.
- [x] Console shows local identities by user-facing name before the raw Jolt
      address.
- [x] Console can delete generated local identities while preserving the daemon
      signing identity.
- [x] App approval prompts show which identity the app is requesting access to.
- [x] Published paths and inventory views are scoped to the selected identity.
- [x] Network settings that are node-level remain node-level, not accidentally
      per-identity.
- [x] Tests cover identity selection for at least one admin/API path and one app
      session path.

## Non-Goals

- Device authorization records.
- Identity import/export redesign.
- Cross-device sync.
- Petnames or social contact management.

## Notes

This card should avoid turning identity switching into app semantics. Profiles,
feeds, posts, pastes, and similar concepts remain app-owned content above the
protocol layer.

## Implementation Notes

- Added admin local identity APIs for listing, creating, and selecting local
  identities.
- Added `DELETE /admin/v1/identities/{identity}` for generated identities. The
  daemon signing identity is protected and deletion of the active generated
  identity falls back to the daemon/default identity.
- Console now presents local identities as a table with name, address, type,
  status, and actions. Identity creation requires a name, and generated
  identities can be assumed or deleted from the table.
- Console filters published inventory to the active identity.
- App approval defaults missing approval identity to the selected local identity
  and shows that active identity in approval prompts.
- Publish, inventory, encryption, decrypt, and pin capabilities still require
  the daemon-signing identity. Generated local identities can be selected for
  non-signing grants now; full publish authority for multiple local identities
  belongs with the daemon/device writer work.

## Verification

- `cargo test -p jolt-server identity --test api_integration -- --nocapture`
- `npx tsc --noEmit` from `apps/jolt-console`
- `npx vitest run src/daemon/client.test.ts src/sections/sections.test.tsx src/app/App.test.tsx`
- `npm test` from `apps/jolt-console`
- `CARGO_TARGET_DIR=/tmp/jolt-tauri-target cargo check -p jolt-console`
- `./scripts/test-local.sh`
