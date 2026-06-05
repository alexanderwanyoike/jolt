# 065: Console Diagnostics and Dashboard Removal

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Implemented in PR
**Blocked by:** 063

## Why

Card 063 removed the old dashboard from the daemon root, but kept the daemon
HTML dashboard available as a debug-only page. That avoided product confusion,
but left a second UI path in the codebase.

Jolt Console should own daemon troubleshooting. The old daemon-served dashboard
should be removed once Console exposes the remaining useful diagnostics.

## What Was Built

- Console daemon snapshots now load connected peers and cache entries from the
  existing daemon APIs.
- Console Diagnostics renders connected peer inventory and cache entry
  inventory alongside the raw status/cache JSON.
- The daemon root remains a minimal pointer to Jolt Console.
- The old daemon-served dashboard HTML file was deleted.
- `/dashboard` and `/debug/dashboard` are no longer routed.

## Acceptance Criteria

- [x] Console Diagnostics shows connected peers.
- [x] Console Diagnostics shows cache entries.
- [x] The daemon root no longer links to a debug dashboard.
- [x] `/dashboard` and `/debug/dashboard` no longer serve the old dashboard.
- [x] The old dashboard HTML code is removed.
- [x] Tests cover the Console diagnostics replacement and retired routes.

## Notes

This intentionally does not move mutating protocol smoke-test forms into
Console. Publish/fetch/resolve remain available through the daemon APIs, CLI,
and app flows; Console Diagnostics is for troubleshooting visibility.

## Verification

- Red: `npx vitest run src/daemon/client.test.ts src/sections/sections.test.tsx`
  failed before Console loaded/rendered peer and cache-entry inventories.
- Green: `npx vitest run src/daemon/client.test.ts src/sections/sections.test.tsx`.
- Red: `cargo test -p jolt-server dashboard --test api_integration -- --nocapture`
  failed before the daemon debug dashboard routes were removed.
- Green: `cargo test -p jolt-server dashboard --test api_integration -- --nocapture`.
- Green: `npx vitest run`.
- Green: `npm test` in `apps/jolt-console`.
- Green: `npm run build` in `apps/jolt-console`.
- Green: `cargo check -p jolt-server`.
- Green: `./scripts/test-local.sh`.
