# 061: Console Network Settings v0

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Ready  
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

- [ ] Console Settings can list configured bootstrap relays.
- [ ] Console Settings can add and remove bootstrap relays.
- [ ] Invalid bootstrap multiaddrs are rejected with visible errors.
- [ ] Console Settings can show, set, and clear the home relay.
- [ ] Invalid home relay multiaddrs/API URLs are rejected with visible errors.
- [ ] Built-in/default, configured, learned, and effective relay concepts are
      not collapsed into one confusing list.
- [ ] External apps cannot change bootstrap or home relay settings.
- [ ] Tests cover daemon admin validation and Console settings interactions.

## Notes

The CLI commands already contain useful validation and persistence behavior.
Reuse shared validation paths where practical instead of reimplementing string
checks in the UI.
