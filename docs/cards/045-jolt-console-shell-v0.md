# 045: Jolt Console Tauri Shell v0

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Done
**Blocked by:** None

## Why

Jolt needs a proper local control application. The daemon is the protocol/runtime process, but the Console is the user-facing trust surface: it is where users understand their node, identity, app permissions, relay state, published content, and diagnostics.

The current localhost dashboard is useful as a temporary debug surface, but it should not become the production Console. The production Console should feel like a real desktop control center that can live in the tray/taskbar and make daemon state understandable without asking users to operate raw HTTP APIs or CLI commands.

## What to Build

Create a first Tauri-based Jolt Console application in this repo.

Recommended location:

```text
apps/jolt-console/
```

This app is part of Jolt's architecture and belongs in this repository while the daemon API is still evolving. External apps such as Pastey remain outside this repository in `jolt-apps`.

For v0, build a small but polished shell:

- Desktop window launched as a local control app.
- Tray/taskbar presence if practical in v0.
- Connection to the local Jolt daemon over the existing localhost API.
- Overview section for daemon health.
- Current identity section.
- Network/relay state section.
- App sessions section placeholder for [046](046-app-permission-approval-ui.md).
- Published/cache section placeholder or basic read-only inventory.
- Diagnostics section placeholder.

Do not spend significant effort redesigning the existing static dashboard. It can remain as an emergency/debug page until the Tauri Console replaces it.

Suggested sections:

- Overview
- Identity
- Apps
- Network
- Relays
- Published
- Cache
- Settings
- Diagnostics

## Acceptance Criteria

- [x] `apps/jolt-console` exists as a Tauri app in this repo.
- [x] Console can connect to a running local Jolt daemon.
- [x] Console shows daemon health and connection state.
- [x] Console shows current `.jolt` identity and peer ID.
- [x] Console shows basic network, relay, published, and cache counts from existing APIs.
- [x] Console has clear navigation sections for Overview, Identity, Apps, Network, Relays, Published, Cache, Settings, and Diagnostics.
- [x] Apps section exists as a placeholder for permission approval work in [046](046-app-permission-approval-ui.md).
- [x] Existing localhost dashboard remains reachable as a temporary debug page.
- [x] No new remote attack surface is exposed by default.
- [x] PR description explains that this is the production Console direction, not a static dashboard rewrite.

## Notes

The Console is not a Jolt app in the same sense as Pastey or Drops. It is a first-party control surface for the daemon.

Keep v0 focused. The goal is not to implement every setting yet; the goal is to establish the production shell and architecture so permission approval can be built in the right place.

## Result

Implemented a first Tauri/Vite/React/TypeScript Console shell under
`apps/jolt-console`. The shell connects to the local daemon through a Tauri
command rather than direct browser fetches, so v0 does not require opening a new
remote API surface.

The Console currently provides read-only overview, identity, app placeholder,
network, relay, published, cache, settings, and diagnostics sections. The app
permission approval flow remains the follow-up in [046](046-app-permission-approval-ui.md).
The frontend is split into route definitions, daemon client/state boundaries,
shared UI primitives, and section components so the Console can absorb future
features without layering everything into one entrypoint.

Verification:

```bash
npm test
npm run build
cargo check -p jolt-console
./scripts/test-local.sh
```
