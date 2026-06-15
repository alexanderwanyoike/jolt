# 104: Jolt Website Strategy

**Type:** HITL
**Milestone:** Project Communication
**Status:** Discussion next
**Blocked by:** None

## Why

Jolt needs a public home that explains the project without requiring people to
read the source tree or infer the thesis from implementation cards.

The website should make the product and protocol legible:

```text
Jolt is a user-owned and community-owned substrate for apps.
Apps give purpose.
Communities give discovery.
Identities give ownership.
Jolt provides signed state, transport, encryption, and availability.
```

## What to Decide

- Decide the website's first audience:
  - curious users;
  - app developers;
  - protocol contributors;
  - relay/community operators.
- Decide the first site structure.
- Decide whether the site should be static docs-first, marketing-first, or a
  hybrid.
- Decide where RFCs live and how the website links them.
- Decide how install instructions, demo apps, limitations, and project status
  stay current.
- Decide whether the website ships from this repo or a separate repo.

## Acceptance Criteria

- [ ] The website has a clear primary audience and secondary audiences.
- [ ] The homepage can explain Jolt without protocol-first jargon.
- [ ] The site structure includes project status, install/demo paths, app model,
      community discovery, RFCs, and limitations.
- [ ] The strategy states how website content stays aligned with repo docs and
      cards.
- [ ] The strategy avoids claiming production maturity before the identity,
      device, community, and encryption work is real.

## Non-Goals

- Building the site.
- Designing a logo or full brand system.
- App store/catalog work.
- Hiding rough edges or limitations.

## Notes

This card should settle the message before implementation. A polished site with
unclear positioning will make the project harder to understand, not easier.
