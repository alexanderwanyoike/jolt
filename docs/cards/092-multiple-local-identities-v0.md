# 092: Multiple Local Identities v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 091  
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
- select the active identity for Console views;
- expose the selected identity in app approval flows;
- make identity-scoped state obvious in Console;
- keep daemon/admin APIs explicit about which identity they operate on.

## Acceptance Criteria

- [ ] A local node can hold at least two user identities.
- [ ] Console can switch between local identities without restarting the daemon.
- [ ] App approval prompts show which identity the app is requesting access to.
- [ ] Published paths and inventory views are scoped to the selected identity.
- [ ] Network settings that are node-level remain node-level, not accidentally
      per-identity.
- [ ] Tests cover identity selection for at least one admin/API path and one app
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

