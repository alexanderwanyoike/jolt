# 064: Jolt Distribution and Packaging Design

**Type:** HITL  
**Milestone:** Console Native Daemon UX  
**Status:** Designed in PR
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

## Decision

For v0, keep distribution and lifecycle deliberately simple across Linux,
macOS, and Windows:

- ship one user-facing Jolt Console desktop app;
- bundle the `jolt` daemon binary as a sidecar of that Console app;
- optionally bundle or separately publish the `jolt` CLI, but do not make the
  CLI required for normal Console use;
- run the daemon as a normal per-user child process when Console starts it;
- let Console stop/restart only the daemon process it started;
- treat a daemon started from a terminal, script, or external supervisor as
  externally owned;
- do not add an OS service, privileged helper, system daemon, launch-on-login,
  or tray/menu-bar presence in this slice.

This is intentionally not the final native-product shape. It gives card 060 a
clear, cross-platform lifecycle target without taking on installer services,
background login items, firewall/service prompts, tray APIs, or per-OS service
management too early.

## First Installable Artifact

The first installable artifact should be the Jolt Console desktop application
with a bundled daemon sidecar:

```text
Jolt Console app / installer
  console UI executable
  jolt daemon sidecar
  optional jolt CLI binary
```

The Console should be usable without asking the user to separately install the
daemon or run `jolt start` in a terminal.

CLI-only/dev workflows remain valid. They are a separate operating mode, not
the packaged Console happy path.

## Platform Scope

The same product model must work on Linux, macOS, and Windows, but the first
implementation should avoid platform-specific background-process features:

- Linux: no systemd user service, distro-specific tray dependency, or package
  manager assumption for v0.
- macOS: no launch agent, login item, privileged helper, or menu-bar-only
  daemon controller for v0.
- Windows: no Windows Service, scheduled task, startup entry, or tray-only
  controller for v0.

Platform-specific installers can come later. The v0 lifecycle contract should
work from a dev build and from a bundled app because it only needs to locate and
spawn a sidecar process.

## Config, Data, And Logs

Use per-user paths, never system-wide privileged paths, for the v0 packaged
Console:

- config: platform standard user config directory for Jolt;
- data/store: platform standard user data directory for Jolt;
- logs: platform standard user log/cache directory where available, otherwise a
  Jolt-controlled per-user directory;
- process metadata: per-user runtime/config state that records only enough to
  identify a Console-owned daemon.

The daemon and CLI should keep sharing the same config/data model where
possible. Packaged Console may pass explicit paths to its sidecar so it does
not depend on shell environment setup.

Path migration is out of scope for this card unless an existing path would
break packaged Console startup.

## Daemon Startup Contract

Card 060 may assume this lifecycle model:

1. Console checks the configured local daemon URL.
2. If a compatible daemon is already running, Console connects and marks it as
   externally owned unless it can prove it started that exact process.
3. If no daemon is running, Console starts the bundled sidecar as a per-user
   child process.
4. Console records enough local process metadata to distinguish its own sidecar
   from an external daemon.
5. Console exposes stop/restart only for Console-owned sidecars.
6. Startup errors and daemon logs are visible from Console.

The daemon remains a separate process and still owns keys, storage, network
state, and app-session authority. The Console remains the privileged native
control surface.

## App Discovery

For v0, external apps discover the daemon the same way Pastey does today: by a
configured/default local daemon URL and the app-session approval API.

Do not add OS registry entries, background brokers, protocol handlers, or a
global app-discovery service for this card. A richer discovery mechanism can be
designed later if app distribution needs it.

## Updates

For v0, avoid hot-swapping a running daemon binary.

If a packaged Console update changes the bundled sidecar, Console should detect
that the currently running daemon is either:

- compatible and can keep running; or
- incompatible and needs a visible restart prompt.

Automatic in-place daemon replacement while it is serving apps is deferred.

## Deferred

- OS service / system daemon installation.
- Launch-on-login / autostart.
- Tray, menu-bar, or taskbar-only daemon presence.
- Privileged helper processes.
- Multi-user machine-wide daemon ownership.
- Automatic daemon binary hot-swap during app update.
- Platform-specific installer policy beyond "can bundle and spawn sidecar".
- OS-level local daemon discovery for third-party apps.

## What to Decide

- [x] Target packaging order: keep the v0 lifecycle cross-platform and avoid
      OS-specific service/tray features until later.
- [x] Whether Tauri sidecars are the first packaging route: yes, treat the
      daemon as a Console sidecar for the first packaged path.
- [x] Where bundled daemon and CLI binaries live inside the app bundle: the
      daemon is bundled with Console; CLI may be bundled or released separately.
- [x] How config/data/log paths are selected and migrated: use per-user
      platform standard Jolt paths; migration is deferred unless needed for
      sidecar startup.
- [x] Whether Console supports user login/autostart on boot: no for v0.
- [x] How external apps discover the local daemon URL: configured/default local
      daemon URL, same as current app-session clients.
- [x] How updates affect a running daemon: no hot-swap; prompt for restart when
      compatibility requires it.

## Acceptance Criteria

- [x] A design note chooses the v0 distribution shape.
- [x] The design keeps daemon and Console as separate processes.
- [x] The design states what is bundled in the first installable artifact.
- [x] The design covers config/data/log path expectations.
- [x] The design covers how Console starts the daemon sidecar.
- [x] The design identifies what is deferred for later platforms or installers.

## Verification

- Docs-only design decision; no code tests were run.

## Notes

This does not need to produce release artifacts immediately. It should give
card 060 enough certainty to implement daemon lifecycle behavior without
painting packaging into a corner.
