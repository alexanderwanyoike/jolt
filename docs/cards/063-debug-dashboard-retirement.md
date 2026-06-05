# 063: Debug Dashboard Retirement

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Ready after 061  
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

- [ ] The daemon root no longer presents the old dashboard as the product UI.
- [ ] Any remaining debug dashboard is clearly labeled debug-only.
- [ ] Docs point users to Jolt Console as the control surface.
- [ ] Existing API behavior remains available for tests and scripts.
- [ ] Tests cover the chosen routing/gating behavior.

## Notes

Do not delete useful diagnostics before Console has a replacement path. The goal
is to remove product confusion, not to remove developer observability.
