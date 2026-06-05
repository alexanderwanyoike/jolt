# 055: Jolt Console Native Daemon UX Debt

**Type:** HITL  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Split into follow-up cards  
**Blocked by:** 045, 046, 047

## Why

Jolt Console should feel like a native control center for a real local daemon,
not a page that the user has to manually refresh or operate like a developer
dashboard.

Manual Pastey approval testing proved the app-session flow works, but it also
exposed the next layer of product debt: daemon state should update live, the
Console should manage daemon lifecycle, and app permission prompts should feel
like OS-native local trust prompts.

## Tech Debt Items

- Make Console realtime enough that users do not need to press Refresh for
  routine daemon/app-session state changes.
- Let Console start the local Jolt daemon when it is not running, similar to a
  Docker Desktop style control app.
- Give Console a proper OS taskbar/tray/sidebar presence so users understand
  Jolt is a real local daemon with ongoing state.
- When an external app requests access, bring Jolt Console to the foreground
  and focus the pending permission request.
- Move bootstrap relay and home relay configuration into Console Settings.
- Decide how Jolt is distributed as a user-installable product, preferably as
  one installed app bundle that includes the Console, daemon sidecar, and CLI.
- Retire or clearly demote the old daemon-served debug dashboard once Console
  covers the user-facing control surfaces.

## Follow-up Cards

- [059](059-console-realtime-state-v0.md): make Console state update without
  manual refresh.
- [060](060-console-daemon-lifecycle-v0.md): let Console start and supervise
  the local daemon without blurring daemon ownership.
- [061](061-console-network-settings-v0.md): manage bootstrap and home relay
  configuration from Console Settings.
- [062](062-console-native-presence-and-permission-focus-v0.md): add tray/native
  presence and focus Console when app permissions are requested.
- [063](063-debug-dashboard-retirement.md): remove, gate, or demote the old
  daemon-served dashboard after Console covers the same user workflows.
- [064](064-jolt-distribution-packaging-design.md): choose the installer/binary
  distribution shape for Console, daemon sidecar, and CLI.

## Acceptance Criteria

- [ ] Console receives daemon state updates without manual refresh for core
      status, app requests, app sessions, and session revocations.
- [ ] Console can start the daemon if no compatible local daemon is running.
- [ ] Console exposes a clear native OS presence for daemon status and quick
      access.
- [ ] A new app-session request can cause Console to open/focus the Apps
      permission surface.
- [ ] The realtime mechanism does not expose a new unauthenticated remote
      attack surface.

## Notes

Realtime could be implemented with polling, server-sent events, WebSocket, a
Tauri-side timer, or a daemon event channel. Choose deliberately after the
daemon lifecycle model is clearer.

Daemon start/stop UX needs careful ownership rules. Console should not
accidentally kill or replace a daemon the user started manually unless the user
explicitly asks it to.

Configuration is also part of the Console-native direction. Bootstrap relays
and home relay settings are admin-only daemon configuration, not app
capabilities. External apps should never be able to change these settings.
