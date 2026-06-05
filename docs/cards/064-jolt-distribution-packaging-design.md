# 064: Jolt Distribution and Packaging Design

**Type:** HITL  
**Milestone:** Console Native Daemon UX  
**Status:** Ready for design  
**Blocked by:** 045

## Why

Jolt needs a distribution shape before daemon lifecycle work can feel real. A
user should install "Jolt" and get the Console, daemon, and optional CLI without
assembling processes manually.

This is worth thinking about now because it affects daemon lifecycle, config
paths, logs, autostart, updates, and how external apps discover the local app
API.

## Candidate Direction

Prefer one installed product that contains separate executables:

```text
Jolt installer / app bundle
  Jolt Console desktop app
  bundled jolt daemon sidecar
  optional jolt CLI
```

The product is all-in-one from the user's perspective, but the daemon remains a
separate process from Console. That preserves the existing architecture:

```text
Console = privileged native control surface
Daemon  = local authority, keys, storage, network, app API
CLI     = terminal/admin affordance
Apps    = untrusted clients with scoped sessions
```

## What to Decide

- Target packaging order: Linux first, then macOS/Windows, or another order.
- Whether Tauri sidecars are the first packaging route.
- Where bundled daemon and CLI binaries live inside the app bundle.
- How config/data/log paths are selected and migrated.
- Whether Console supports user login/autostart on boot.
- How external apps discover the local daemon URL.
- How updates affect a running daemon.

## Acceptance Criteria

- [ ] A design note chooses the v0 distribution shape.
- [ ] The design keeps daemon and Console as separate processes.
- [ ] The design states what is bundled in the first installable artifact.
- [ ] The design covers config/data/log path expectations.
- [ ] The design covers how Console starts the daemon sidecar.
- [ ] The design identifies what is deferred for later platforms or installers.

## Notes

This does not need to produce release artifacts immediately. It should give
card 060 enough certainty to implement daemon lifecycle behavior without
painting packaging into a corner.
