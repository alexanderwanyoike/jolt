# 103: Spoke Community Recommended Users v0

**Type:** AFK after design  
**Milestone:** Community Discovery Sprint  
**Status:** Ready after 102  
**Blocked by:** 100, 102

## Why

Joining a Spoke community should help users discover people, not just posts.
This gives Spoke a meaningful cold-start loop without global identity search:

```text
join community -> discover people -> follow people -> see posts/replies
```

## What to Build

Add community-scoped user recommendations in Spoke:

- read member/activity summaries from a signed community app index;
- show recommended users from watched/joined communities;
- explain recommendation reasons, such as member, moderator, active poster,
  recent reply, shared topic, or mutual community;
- let users follow recommended identities;
- keep ranking local to Spoke where possible;
- preserve signed evidence for membership and activity.

## Acceptance Criteria

- [ ] Spoke can show recommended identities from a joined community.
- [ ] Recommendations include an explainable reason.
- [ ] Recommendations are based on signed community membership or signed member
      activity.
- [ ] Following a recommended identity uses existing identity/app grant
      boundaries.
- [ ] The recommendation path does not require relay-owned search.
- [ ] Tests or smoke coverage prove recommendations from a community index.

## Non-Goals

- Global recommendation service.
- Opaque platform ranking.
- Social graph import.
- Protocol-level follow semantics.

## Notes

The community can curate an index, but individual user activity should remain
signed by the user identity that produced it.

