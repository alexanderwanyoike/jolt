# 062: Console Native Presence and Permission Focus v0

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Deferred after simple lifecycle
**Blocked by:** 045, 046, 059

## Why

Jolt is a real local daemon with ongoing state. Console should have native OS
presence and permission prompts should feel like local trust decisions, not a
web page the user must remember to check.

## What to Build

Add native Console behavior around daemon/app-session state:

- a tray/taskbar/menu-bar presence where Tauri supports it;
- clear status indication for daemon online/offline/degraded;
- quick action to open/show Console;
- when a new app-session request appears, bring/focus Console and navigate to
  the Apps permission surface;
- avoid repeated focus spam for the same request.

## Acceptance Criteria

- [ ] Console exposes a native OS presence in dev and packaged builds where
      supported.
- [ ] The native presence can open/show the Console window.
- [ ] New app-session requests can focus the Console Apps page.
- [ ] Existing or already-seen requests do not repeatedly steal focus.
- [ ] Focus behavior is tested through app state where direct OS automation is
      not practical.
- [ ] Unsupported platforms degrade gracefully.

## Notes

This card depends on automatic app-request refresh from card 059. Without that,
Console will not reliably know when a new permission request appears.

Card 064 deliberately defers tray/menu-bar/taskbar presence from the simple v0
packaging and daemon lifecycle path. Revisit this after Console can start and
supervise its daemon sidecar without OS service or autostart behavior.
