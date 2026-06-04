# 047: Pastey Uses App Sessions

**Type:** AFK  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Done  
**Blocked by:** 044, 046

## Why

Pastey currently talks to trusted daemon endpoints through a dev proxy. That was useful for proving the app boundary, but it should move to capability-checked app sessions once the daemon supports them.

Pastey is an external Jolt app and should remain outside this repository in `jolt-apps`. This card tracks the cross-repo integration point: the daemon and Jolt Console live here; Pastey changes happen in the app repository.

## What to Build

In `jolt-apps`, update Pastey so it:

- Requests a session on startup.
- Requests capabilities for `/pastes/*`.
- Waits for approval.
- Stores and uses the session token.
- Calls `/app/v1/*` endpoints instead of trusted `/api/v1/*` endpoints.
- Displays pending, approved, rejected, and revoked states.

## Acceptance Criteria

- [x] Fresh Pastey start creates a pending app session request.
- [x] Jolt Console can approve Pastey.
- [x] Pastey switches to ready state after approval.
- [x] Pastey can publish under `/pastes/*` using the app API.
- [x] Pastey can fetch public `.jolt` paste addresses using the app API.
- [x] Pastey cannot publish outside `/pastes/*`.
- [x] Pastey handles revoked sessions clearly.

## Implementation Notes

- Pastey lives in `https://github.com/alexanderwanyoike/pastey`.
- PR: `https://github.com/alexanderwanyoike/pastey/pull/1`.
- Pastey requests a scoped app session for public resolve/fetch plus
  `/pastes/*` publish, inventory, and own-pin authority.
- Pastey uses bearer-token `/app/v1/*` calls for publish, list, fetch, resolve,
  and home-relay pin operations.
- Pastey clears stale stored request IDs when a throwaway daemon no longer knows
  the request, then creates a fresh pending app-session request.

## Verification

- `npm test` in Pastey.
- `npm run build` in Pastey.
- Manual local smoke with Jolt daemon and Jolt Console: request, approve,
  publish/list, revoke, and revoked-state recovery.

## Notes

Pastey remains public-only in this card. Private encrypted pastes come later.

Do not move Pastey into this repository. Jolt Console is first-party daemon architecture; Pastey is a separate app built on top of Jolt.
