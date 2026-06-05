# 059: Console Realtime State v0

**Type:** AFK  
**Milestone:** Console Native Daemon UX  
**Status:** Ready  
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

- [ ] Console updates app permission requests without pressing Refresh.
- [ ] Console updates active/revoked app sessions without pressing Refresh.
- [ ] Console updates daemon status summaries without pressing Refresh.
- [ ] Manual Refresh remains available and does not fight with background
      refresh.
- [ ] Polling pauses or backs off when the daemon is unreachable.
- [ ] Tests cover the refresh loop and state transitions without relying on
      real timers where practical.
- [ ] No new non-local daemon attack surface is introduced.

## Notes

This card should keep the mechanism deliberately boring. A future daemon event
channel can replace polling once lifecycle and packaging are clearer.
