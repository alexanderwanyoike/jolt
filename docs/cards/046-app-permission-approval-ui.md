# 046: App Permission Approval UI

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Ready after 043 and 045  
**Blocked by:** 043, 045

## Why

App sessions need a user-facing approval and revocation flow. Without a console UI, app permissions become either invisible or too developer-only.

## What to Build

Add app permission management to Jolt Console:

- Pending app requests.
- Requested identity.
- Requested capabilities.
- Approve/reject actions.
- Active sessions.
- Revoke action.
- Last-used metadata if available.

The UI should make authority expansion obvious:

```text
Pastey wants to use alice.jolt to publish /pastes/*, fetch public content, and pin own pastes.
```

## Acceptance Criteria

- [ ] Pending app requests are visible in Console.
- [ ] User can approve a request.
- [ ] User can reject a request.
- [ ] Active grants are visible with app name, identity, capabilities, and status.
- [ ] User can revoke an active grant.
- [ ] Revoked grants stop working for app API calls.
- [ ] Dangerous/admin-only operations are not grantable through normal app requests.

## Notes

Prompt for broad categories and path scopes, not every routine paste/fetch operation.
