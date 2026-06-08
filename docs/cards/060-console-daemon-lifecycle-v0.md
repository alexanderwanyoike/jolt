# 060: Console Daemon Lifecycle v0

**Type:** AFK
**Milestone:** Console Native Daemon UX  
**Status:** Implemented in PR
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

- [x] A short design note or implementation PR defines daemon ownership states.
- [x] Console can tell "daemon unavailable" from "daemon running but unhealthy".
- [x] Console can start a local daemon in dev or packaged mode.
- [x] Console does not stop an externally started daemon without explicit user
      intent.
- [x] Startup failures are visible in Console.
- [x] Tests cover process command construction and ownership-state rendering.

## Implementation Notes

Ownership states for v0:

- `none`: no local daemon is responding on the configured local daemon URL.
- `external`: a local daemon endpoint is reachable, but Console did not start
  the process and must not stop or restart it.
- `console`: Console started the daemon sidecar as a normal per-user child
  process and may stop or restart that child.

Reachability states for v0:

- `healthy`: `/api/v1/health` responds successfully.
- `unhealthy`: a local endpoint or Console-owned child exists but health is not
  currently good.
- `unavailable`: no local endpoint is responding and Console does not own a
  child process.

Console uses `JOLT_DAEMON_BINARY` in dev when provided, otherwise it resolves a
sidecar-like `jolt`/`jolt.exe` binary next to the Console executable. The start
command is `jolt start --api-port <configured port> --api-bind 127.0.0.1`.

The sidecar stdout/stderr are written to a per-user temp log path by default
and tailed into the Settings page. `JOLT_CONSOLE_DAEMON_LOG` can override the
log path for dev/manual testing.

## Verification

- Red: `npx vitest run src/sections/sections.test.tsx` failed while Settings
  still rendered the read-only placeholder instead of lifecycle controls.
- Green: `npx vitest run src/sections/sections.test.tsx`.
- Red: `cargo test -p jolt-console daemon_start_plan_uses_configured_binary_and_local_api_port`
  failed while daemon start command planning did not exist.
- Green: `cargo test -p jolt-console --lib`.
- Green: `npx vitest run src/daemon/client.test.ts`.
- Green: `npm test` in `apps/jolt-console`.
- Green: `npm run build` in `apps/jolt-console`.
- Green: `cargo check -p jolt-console`.
- Green: `./scripts/test-local.sh`.

## Notes

This should be settled before deep packaging work. Card 064 should decide the
bundle shape; this card should decide runtime behavior.
