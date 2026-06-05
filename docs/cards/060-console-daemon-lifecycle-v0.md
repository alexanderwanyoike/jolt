# 060: Console Daemon Lifecycle v0

**Type:** AFK
**Milestone:** Console Native Daemon UX  
**Status:** Ready after 064
**Blocked by:** 045, 064

## Why

Jolt Console should feel like the control app for a local daemon, not a UI that
only works after the user separately starts `jolt start` in a terminal.

The tricky part is ownership. Console must not accidentally kill, replace, or
mutate a daemon that the user started manually unless the user explicitly asks
for that behavior.

## What to Decide

Define the lifecycle contract between Console and daemon:

- How Console detects a compatible daemon on the configured local API URL.
- How Console starts a daemon when none is running.
- How Console distinguishes a daemon it owns from a daemon the user started.
- Whether Console can stop/restart only Console-owned daemons.
- Where logs, PID/process metadata, and startup errors are shown.
- How this works in dev mode versus packaged installs with a bundled daemon
  sidecar.

## Candidate Direction

Card 064 chose the simple v0 packaging/lifecycle model:

```text
Jolt Console app bundle
  starts/manages a jolt daemon sidecar as a normal user child process
  does not install an OS service, tray app, launch agent, or autostart entry
  treats terminal/script/supervisor-started daemons as externally owned
```

Console can show controls such as Start, Restart, Stop only when the ownership
state makes them honest.

The implementation should stay cross-platform by relying on sidecar process
management and per-user paths, not Linux systemd units, macOS launch agents, or
Windows Services.

## Acceptance Criteria

- [ ] A short design note or implementation PR defines daemon ownership states.
- [ ] Console can tell "daemon unavailable" from "daemon running but unhealthy".
- [ ] Console can start a local daemon in dev or packaged mode.
- [ ] Console does not stop an externally started daemon without explicit user
      intent.
- [ ] Startup failures are visible in Console.
- [ ] Tests cover process command construction and ownership-state rendering.

## Notes

This should be settled before deep packaging work. Card 064 should decide the
bundle shape; this card should decide runtime behavior.
