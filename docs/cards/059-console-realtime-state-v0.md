# 059: Console Realtime State v0

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Implemented in PR  
**Blocked by:** 045, 046

## Why

Console currently behaves too much like a manually refreshed dashboard. During
Pastey approval testing, app-session changes were easy to miss unless the user
pressed Refresh or waited for the app to retry.

For v0, "realtime" does not need a daemon event protocol. It needs to make the
native control surface feel alive while preserving the daemon's local-only
attack surface.

## What to Build

Add automatic Console refresh for the daemon state already used by the app:

- core daemon status;
- app permission requests;
- active/revoked app sessions;
- published/cache/relay summary data used by the visible sections.

Prefer a conservative Tauri/frontend polling loop first unless an existing
daemon endpoint makes a cleaner event stream obvious. Do not add a remotely
reachable WebSocket or unauthenticated event API for this card.

## Acceptance Criteria

- [x] Console updates app permission requests without pressing Refresh.
- [x] Console updates active/revoked app sessions without pressing Refresh.
- [x] Console updates daemon status summaries without pressing Refresh.
- [x] Manual Refresh remains available and does not fight with background
      refresh.
- [x] Polling pauses or backs off when the daemon is unreachable.
- [x] Tests cover the refresh loop and state transitions without relying on
      real timers where practical.
- [x] No new non-local daemon attack surface is introduced.

## Implementation Notes

- Kept the v0 mechanism local and frontend/Tauri-side: no daemon WebSocket,
  SSE, or new unauthenticated event endpoint.
- `useDaemonSnapshot` now schedules refreshes with a timeout loop and backs off
  after failures.
- `AppsPage` now polls app requests and sessions using the same Console refresh
  interval, performs background refreshes without flashing loading state, skips
  polling while approve/reject/revoke actions are running, and backs off after
  permission API failures.
- Manual Refresh remains available on both the shell and Apps permission page.

## Verification

- Red: `npx vitest run src/sections/sections.test.tsx` failed while app
  permission requests did not update without manual refresh.
- Green: `npx vitest run src/sections/sections.test.tsx`.
- Red: `npx vitest run src/sections/sections.test.tsx` failed while permission
  polling retried immediately after an API failure.
- Green: `npx vitest run src/sections/sections.test.tsx`.
- Red: `npx vitest run src/app/App.test.tsx` failed while daemon status polling
  retried immediately after an API failure.
- Green: `npx vitest run src/app/App.test.tsx`.
- Green: `npm test` in `apps/jolt-console`.
- Green: `npm run build` in `apps/jolt-console`.
- Green: `cargo check -p jolt-console`.
- Green: `./scripts/test-local.sh`.

## Notes

This card should keep the mechanism deliberately boring. A future daemon event
channel can replace polling once lifecycle and packaging are clearer.
