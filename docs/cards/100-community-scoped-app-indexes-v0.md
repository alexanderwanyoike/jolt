# 100: Community-Scoped App Indexes v0

**Type:** AFK after design
**Milestone:** Community Discovery Sprint
**Status:** Ready after 099
**Blocked by:** 096, 099

## Why

Communities become useful when apps can publish and read community-scoped
indexes. This is the discovery mechanism that keeps relays dumb: clients fetch
signed catalogs or feeds from community identities and search/render them
locally.

For example:

```text
community identity
  /apps/spoke/feed -> CID(...)
  /apps/jolt-share/catalog -> CID(...)
```

## What to Build

Add generic support for community-scoped app indexes:

- let an app resolve app-specific indexes under a community identity;
- let accepted members submit signed app entries to a community;
- let an authorized community device/admin include, reject, or remove entries;
- preserve the original member signature inside community indexes;
- support public and member-only encrypted indexes;
- expose enough metadata for apps to search locally after fetching the index.

## Acceptance Criteria

- [ ] A community can publish a signed app index without Jolt understanding the
      app schema.
- [ ] Member-submitted entries remain signed by the submitting member identity.
- [ ] The community index signature proves curation, not authorship of member
      entries.
- [ ] Non-members cannot submit entries to member-only community indexes.
- [ ] Member-only indexes can be encrypted to accepted members/devices.
- [ ] Apps can fetch a followed/joined community index and search or render it
      locally.
- [ ] Tests cover public index, member-only index, accepted submission, rejected
      submission, and signature verification.

## Non-Goals

- Relay-owned search.
- Global query propagation.
- App-specific ranking algorithms.
- Protocol-level posts, feeds, files, or pastes.

## Notes

Search should be:

```text
fetch signed community/app indexes;
verify entries;
search locally;
fetch selected content by CID.
```
