# 046: App Permission Approval in Jolt Console

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Implemented in PR
**Blocked by:** None

## Why

App sessions need a user-facing approval and revocation flow. Without the Jolt Console, app permissions become either invisible or too developer-only.

This is the first serious trust-surface workflow in Jolt. When an external app asks to act as a `.jolt` identity, the user must be able to see exactly what authority is being granted and revoke it later.

## What to Build

Add app permission management to the Tauri Jolt Console from [045](045-jolt-console-shell-v0.md).

- Pending app requests.
- Requested identity.
- Requested capabilities.
- Approve/reject actions.
- Active sessions.
- Revoke action.
- Last-used metadata if available.
- Clear status for pending, active, rejected, revoked, and expired grants.

The UI should make authority expansion obvious:

```text
Pastey wants to use alice.jolt to publish /pastes/*, fetch public content, and pin own pastes.
```

For v0, use the existing daemon endpoints:

```text
GET  /admin/v1/app-requests
POST /admin/v1/app-requests/{request_id}/approve
POST /admin/v1/app-requests/{request_id}/reject
GET  /admin/v1/app-sessions
POST /admin/v1/app-sessions/{session_id}/revoke
```

## Acceptance Criteria

- [x] Pending app requests are visible in Console.
- [x] User can approve a request.
- [x] User can reject a request.
- [x] Active grants are visible with app name, identity, capabilities, and status.
- [x] User can revoke an active grant.
- [x] Revoked grants stop working for app API calls.
- [x] Dangerous/admin-only operations are not grantable through normal app requests.
- [x] UI copy distinguishes routine public read capabilities from identity-scoped write/pin capabilities.
- [x] Broad path scopes are visually obvious.
- [x] Console continues to work if there are no pending requests or sessions.

## Implementation Notes

- Replaced the Console Apps placeholder with a live permission view backed by
  `GET /admin/v1/app-requests` and `GET /admin/v1/app-sessions`.
- Added Console actions for approve, reject, refresh, and revoke through a new
  Tauri `daemon_post` bridge.
- Rendered public read capabilities separately from identity-scoped publish,
  inventory, and pin authority, with wildcard path scopes called out.
- Disabled approval for requests that include non-grantable/admin-only
  capabilities.
- Added daemon-side app session grant validation so forbidden capability strings
  cannot be approved even if a client bypasses Console.
- Follow-up UI density pass: sorted pending requests and sessions from newest
  to oldest, changed permission grants into compact accordion rows, kept
  approve/reject/revoke actions available from the row header, and made the
  Console shell use the maximized window width.

## Verification

- Red: added focused Console tests for permission loading/actions and a daemon
  regression test showing forbidden capabilities were previously approved.
- Green:
  - `npx vitest run src/daemon/client.test.ts src/sections/sections.test.tsx`
  - `npm test` in `apps/jolt-console`
  - `npm run build` in `apps/jolt-console`
  - `cargo test -p jolt-server app_session`
  - `cargo test -p jolt-server test_admin_cannot_approve_forbidden_app_session_capabilities`
  - `cargo fmt --check`
  - `./scripts/test-local.sh`
- Browser smoke: headless Chrome rendered `http://127.0.0.1:1420/#/apps`
  with the Apps empty/error states usable and no visible overlap. Plain browser
  mode cannot exercise Tauri `invoke`, so the live admin API path remains covered
  by Vitest and the TypeScript build.
- Follow-up UI checks:
  - `npx vitest run src/sections/sections.test.tsx`
  - `npm test` in `apps/jolt-console`
  - `npm run build` in `apps/jolt-console`

## Notes

Prompt for broad categories and path scopes, not every routine paste/fetch operation.

This should not be implemented in the temporary static dashboard unless a minimal bridge is needed for local testing. The intended product surface is the Tauri Console.
