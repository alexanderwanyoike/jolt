# 047: Pastey Uses App Sessions

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Ready after 044 and 046  
**Blocked by:** 044, 046

## Why

Pastey currently talks to trusted daemon endpoints through a dev proxy. That was useful for proving the app boundary, but it should move to capability-checked app sessions once the daemon supports them.

## What to Build

Update Pastey so it:

- Requests a session on startup.
- Requests capabilities for `/pastes/*`.
- Waits for approval.
- Stores and uses the session token.
- Calls `/app/v1/*` endpoints instead of trusted `/api/v1/*` endpoints.
- Displays pending, approved, rejected, and revoked states.

## Acceptance Criteria

- [ ] Fresh Pastey start creates a pending app session request.
- [ ] Jolt Console can approve Pastey.
- [ ] Pastey switches to ready state after approval.
- [ ] Pastey can publish under `/pastes/*` using the app API.
- [ ] Pastey can fetch public `.jolt` paste addresses using the app API.
- [ ] Pastey cannot publish outside `/pastes/*`.
- [ ] Pastey handles revoked sessions clearly.

## Notes

Pastey remains public-only in this card. Private encrypted pastes come later.
