# 102: Spoke Community Join v0

**Type:** AFK after design
**Milestone:** Community Discovery Sprint
**Status:** Ready after 100 and 101
**Blocked by:** 095, 099, 100, 101

## Why

Spoke needs a better cold start than manually entering identities. Communities
give users a social discovery surface:

```text
install Jolt;
install Spoke;
watch or join a community;
discover posts and people through that community.
```

## What to Build

Implement the first Spoke community path:

- list watched/joined Spoke-compatible communities;
- show public community profile/policy;
- join an open community from Spoke;
- submit a signed Spoke post into a community app index when membership allows;
- read a community-scoped Spoke feed from a signed community app index;
- clearly distinguish public watching from accepted membership.

## Acceptance Criteria

- [ ] Spoke can show default or manually added communities.
- [ ] Spoke can join an open community through Jolt membership APIs.
- [ ] Spoke can show whether the current identity is watching, pending, joined,
      rejected, or revoked.
- [ ] A joined member can submit a signed post to a community feed/index.
- [ ] Spoke can render a community feed without Jolt protocol code
      understanding posts.
- [ ] Non-members cannot submit to member-only community feeds.
- [ ] Tests or smoke coverage prove the join, post, and read path.

## Non-Goals

- Full moderation UI.
- Global identity search.
- Ranking/recommendation algorithms.
- Browser support.

## Notes

This is the first product loop for:

```text
apps give purpose;
communities give discovery.
```
