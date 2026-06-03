# 046: App Permission Approval in Jolt Console

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Ready  
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

- [ ] Pending app requests are visible in Console.
- [ ] User can approve a request.
- [ ] User can reject a request.
- [ ] Active grants are visible with app name, identity, capabilities, and status.
- [ ] User can revoke an active grant.
- [ ] Revoked grants stop working for app API calls.
- [ ] Dangerous/admin-only operations are not grantable through normal app requests.
- [ ] UI copy distinguishes routine public read capabilities from identity-scoped write/pin capabilities.
- [ ] Broad path scopes are visually obvious.
- [ ] Console continues to work if there are no pending requests or sessions.

## Notes

Prompt for broad categories and path scopes, not every routine paste/fetch operation.

This should not be implemented in the temporary static dashboard unless a minimal bridge is needed for local testing. The intended product surface is the Tauri Console.
