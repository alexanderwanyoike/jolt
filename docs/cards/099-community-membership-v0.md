# 099: Community Membership v0

**Type:** AFK after design
**Milestone:** Community Discovery Sprint
**Status:** Ready after 098
**Blocked by:** 093, 098

## Why

Communities need a real membership path. Users should not silently become
members of default communities, but open communities should still be easy to
join.

Membership should be generic Jolt state that apps can use without Jolt protocol
code knowing what the community is for.

## What to Build

Implement the minimal generic community membership path:

- publish community join policy under a community identity;
- watch a community's public state locally;
- send a signed join request to a community identity;
- auto-accept open community joins;
- list and decide request-based joins through community-admin/device authority;
- publish signed membership grants;
- publish signed membership revocations;
- expose membership state to app sessions.

## Acceptance Criteria

- [ ] A user can watch a public community identity without joining it.
- [ ] A user can join an open community and receive a signed membership grant.
- [ ] A user can request membership in a request-based community.
- [ ] An authorized community admin/device can accept or reject a join request.
- [ ] A community can revoke a membership grant.
- [ ] App APIs can tell whether the session identity is a member of a community.
- [ ] Tests cover open join, request join, accept, reject, revoke, and
      unauthorized membership mutation.

## Non-Goals

- Spoke-specific community UI.
- Global community directory.
- Payment, reputation, or storage-market mechanics.
- Social graph ranking.

## Notes

Open join should still produce a community-signed membership record so bans and
revocations have a clear source of truth.
