# 043: App Session Store and Approval API

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Done
**Blocked by:** 042

## Why

External apps need stable, revocable authority to ask the daemon to act. The daemon needs a persistent session store so app approval is not just an in-memory experiment.

## What to Build

Add daemon/server support for:

- Pending app session requests.
- Approved app sessions.
- Rejected/revoked sessions.
- Session token generation and lookup.
- Session expiry metadata.
- Persistence across daemon restarts.

The API should support:

```text
POST /app/v1/sessions/request
GET  /app/v1/sessions/{request_id}
GET  /admin/v1/app-requests
POST /admin/v1/app-requests/{request_id}/approve
POST /admin/v1/app-requests/{request_id}/reject
GET  /admin/v1/app-sessions
POST /admin/v1/app-sessions/{session_id}/revoke
GET  /app/v1/session
```

Exact paths can change if [042](042-app-boundary-session-design.md) chooses different names.

## Acceptance Criteria

- [x] Apps can create a pending session request with app identity, display name, requested identity, and requested capabilities.
- [x] Admin API can list pending requests.
- [x] Admin API can approve/reject pending requests.
- [x] Approved sessions receive a token.
- [x] Admin API can list and revoke sessions.
- [x] Revoked sessions cannot be used.
- [x] Session state survives daemon restart.
- [x] Tests cover request, approve, reject, revoke, and restart persistence.

## Result

- Added a persistent app session store under the daemon data directory.
- Added app request, app polling, admin approval/rejection, admin session list, revocation, and bearer-token session introspection endpoints.
- Session tokens are only returned to the app approval flow; persisted state stores token hashes and admin responses do not expose tokens or token hashes.
- Revoked or expired session tokens are rejected by `GET /app/v1/session`.
- Added integration tests covering the full request/approve/reject/revoke/persist/revoked-token flow.

## Notes

Do not expose private keys to apps. Session tokens authorize daemon actions; they are not identity keys.
