# 063: Debug Dashboard Retirement

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Implemented in PR
**Blocked by:** 045, 046, 061

## Why

The old daemon-served dashboard was useful for proving early protocol flows,
but Jolt Console is now the intended product control surface. Keeping two
parallel UIs risks split behavior, stale features, and confusing user guidance.

The old dashboard may still be useful as an emergency debug page during
development, but it should no longer look like the product.

## What to Build

Choose and implement one of:

- remove the daemon-served dashboard entirely;
- move it behind an explicit debug route/name;
- gate it behind a dev/debug flag;
- replace its root entry with a minimal pointer to Jolt Console.

Do this only after Console covers the user workflows that the dashboard still
serves: status, relays/settings, published content, cache, diagnostics, and app
permissions.

## Acceptance Criteria

- [x] The daemon root no longer presents the old dashboard as the product UI.
- [x] Any remaining debug dashboard is clearly labeled debug-only.
- [x] Docs point users to Jolt Console as the control surface.
- [x] Existing API behavior remains available for tests and scripts.
- [x] Tests cover the chosen routing/gating behavior.

## Notes

Do not delete useful diagnostics before Console has a replacement path. The goal
is to remove product confusion, not to remove developer observability.

## Implementation Notes

- The daemon root now serves a minimal Jolt Console pointer page.
- The old dashboard is retained at `/debug/dashboard` and labeled
  `Jolt Debug Dashboard` / `debug-only`.
- `/dashboard` redirects to `/debug/dashboard` for old bookmarks.
- No `/api/v1/*`, `/admin/v1/*`, or `/app/v1/*` behavior was changed.

Follow-up: [065](065-console-diagnostics-and-dashboard-removal.md) moved the
remaining useful diagnostics into Console and removed the old daemon dashboard
HTML/routes entirely.

## Verification

- Red: `cargo test -p jolt-server dashboard --test api_integration -- --nocapture`
  failed while `/` still served the old dashboard.
- Green: `cargo test -p jolt-server dashboard --test api_integration -- --nocapture`.
- Green: `cargo check -p jolt-server`.
- Green: `./scripts/test-local.sh`.
