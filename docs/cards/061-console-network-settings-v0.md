# 061: Console Network Settings v0

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Implemented in PR
**Blocked by:** 045, 023, 029

## Why

Bootstrap relay configuration and home relay configuration are core node
settings. Today they are mostly CLI-oriented, which makes Jolt feel like a dev
tool rather than a native local control app.

These settings are admin-only daemon configuration. They must not become app
capabilities.

## What to Build

Move the existing configuration workflows into Console Settings:

- show configured bootstrap relays;
- show built-in/default relay usage separately from user-configured relays;
- show effective bootstrap relay count and current bootstrap health;
- add/remove configured bootstrap relays with validation;
- show current home relay multiaddr and optional API URL;
- set/clear home relay with validation;
- show home relay pin/availability state where existing APIs already expose it.

If daemon admin endpoints are missing, add local admin-only endpoints rather
than shelling out to the CLI from the frontend.

## Acceptance Criteria

- [x] Console Settings can list configured bootstrap relays.
- [x] Console Settings can add and remove bootstrap relays.
- [x] Invalid bootstrap multiaddrs are rejected with visible errors.
- [x] Console Settings can show, set, and clear the home relay.
- [x] Invalid home relay multiaddrs/API URLs are rejected with visible errors.
- [x] Built-in/default, configured, learned, and effective relay concepts are
      not collapsed into one confusing list.
- [x] External apps cannot change bootstrap or home relay settings.
- [x] Tests cover daemon admin validation and Console settings interactions.

## Notes

The CLI commands already contain useful validation and persistence behavior.
Reuse shared validation paths where practical instead of reimplementing string
checks in the UI.

## Implementation Notes

- Added admin-only daemon endpoints for reading network settings, adding/removing
  configured bootstrap relays, and setting/clearing the home relay.
- Console Settings now shows configured relays, built-in defaults, effective
  startup state, runtime bootstrap health/learned relay counts, and home relay
  details.
- Network settings writes preserve unrelated/future keys in `config.json`.
- External apps have no `/app/v1/*` route for changing bootstrap or home relay
  settings.

## Verification

- Red: `cargo test -p jolt-server test_admin_network_settings_can_update_bootstrap_and_home_relay --test api_integration -- --nocapture` failed while unknown config keys were clobbered.
- Green: `cargo test -p jolt-server test_admin_network_settings_can_update_bootstrap_and_home_relay --test api_integration -- --nocapture`.
- Green: `npx vitest run src/daemon/client.test.ts src/sections/sections.test.tsx`.
- Green: `npm test` in `apps/jolt-console`.
- Green: `npm run build` in `apps/jolt-console`.
- Green: `cargo check -p jolt-server -p jolt-network`.
- Green: `./scripts/test-local.sh`.
