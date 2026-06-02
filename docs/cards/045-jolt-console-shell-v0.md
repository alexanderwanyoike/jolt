# 045: Jolt Console Shell v0

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Ready after 042  
**Blocked by:** 042

## Why

The daemon needs a proper local control UI. The existing dashboard is useful, but Jolt now needs a console for daemon status, identities, relays, app sessions, permissions, published content, and diagnostics.

## What to Build

Turn the current localhost dashboard/control surface into a sidebar shell.

Sections:

- Overview
- Identities
- Apps
- Network
- Relays
- Published
- Cache
- Settings
- Diagnostics

For v0, sections can reuse existing API data and include explicit placeholders for features that are not implemented yet.

## Acceptance Criteria

- [ ] The console is served locally by the daemon.
- [ ] Sidebar navigation works without a frontend build step.
- [ ] Existing dashboard functionality remains reachable.
- [ ] Overview shows daemon state, identity address, peer counts, relay state, and published/cache counts.
- [ ] Apps section exists and explains app sessions if [043](043-app-session-store-approval-api.md) is not implemented yet.
- [ ] Identities section shows the current identity and clearly marks multi-identity management as future work if not implemented.
- [ ] UI remains localhost-first and does not expose a new remote attack surface by default.

## Notes

This is a web UI implementation, but the daemon is not a web app. The console is a local control surface for a Rust daemon.
