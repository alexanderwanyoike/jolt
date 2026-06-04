# 042: App Boundary and Session Design

**Type:** HITL  
**Milestone:** App Boundary / Private Sharing Foundations  
**Status:** Reviewed in PR
**Blocked by:** None

## Why

Pastey proved that external apps can consume Jolt through the daemon, but it also exposed the unsafe part of the current model: any localhost app can call the daemon's trusted API.

Jolt needs a clear boundary:

```text
daemon = local authority, identities, keys, network
apps   = untrusted clients with scoped sessions
console = privileged local control surface
```

## What to Decide

Write a design note covering:

- App session lifecycle: requested, pending, approved, rejected, revoked, expired.
- Capability vocabulary for v0 app actions.
- Which endpoints are app-facing versus console/admin-only.
- How apps request identity use without receiving private keys.
- How sessions are pinned to identities and path scopes.
- Which operations normal apps must never receive.
- Whether legacy `/api/v1/*` remains trusted/dev-only while `/app/v1/*` becomes capability checked.

## Acceptance Criteria

- [x] A design note exists under `docs/`.
- [x] It defines app sessions, app grants, capabilities, and revocation.
- [x] It explicitly separates app APIs from console/admin APIs.
- [x] It lists forbidden normal-app capabilities such as exporting keys or deleting identities.
- [x] It defines v0 capabilities for Pastey: resolve, fetch, publish `/pastes/*`, inventory `/pastes/*`, and pin own content.
- [x] Human review confirms the direction before implementation cards begin.

## Notes

Prompt for authority expansion, not routine use. Pastey should not ask on every paste once `/pastes/*` has been granted.

## Result

Design note: [App Boundary and Sessions](../15-app-boundary-and-sessions.md).

Implementation cards [043](043-app-session-store-approval-api.md) and
[044](044-capability-checked-app-api-v0.md) have landed from this design. This
card was reviewed after Pastey and the Console exercised the model locally.

## Review Outcome

Human review accepted the daemon/app/console boundary with one required
hardening slice before private Pastey work: [056](056-app-capability-grant-hardening.md).

Card 056 tightens app grant validation so approved capabilities must be exactly
what the app requested or a narrower path scope. It also rejects malformed path
wildcards before private `encrypt`, `decrypt`, and `share` capabilities are
introduced.
