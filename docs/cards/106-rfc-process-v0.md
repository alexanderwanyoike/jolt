# 106: RFC Process v0

**Type:** HITL
**Milestone:** Project Communication
**Status:** Discussion next
**Blocked by:** None

## Why

Jolt is accumulating protocol and product decisions that should not live only in
chat, work cards, or PR descriptions. The project needs a lightweight RFC
process for decisions that shape protocol behavior, app boundaries, identity,
communities, encryption, and compatibility.

RFCs should be durable design records, not heavyweight governance theater.

## What to Decide

- Decide what requires an RFC.
- Decide RFC states, such as:
  - draft;
  - accepted;
  - implemented;
  - superseded;
  - rejected.
- Decide the RFC template.
- Decide numbering and file naming.
- Decide review expectations before implementation cards are opened.
- Decide how RFCs relate to existing `docs/*.md`, `docs/cards/*.md`, and PR
  descriptions.
- Decide whether RFCs are protocol-only or also cover product/app contracts.

## Acceptance Criteria

- [ ] The repo has a documented RFC lifecycle.
- [ ] The repo has an RFC template.
- [ ] The process is lightweight enough for a solo/small project.
- [ ] The process says which changes require an RFC.
- [ ] The process says how RFCs are superseded or amended.
- [ ] The process links RFCs to implementation cards without replacing cards.

## Non-Goals

- Formal standards-body governance.
- Voting mechanics.
- Public forum infrastructure.
- Rewriting all existing docs immediately.

## Notes

Good RFC candidates include true multi-writer identities, device authorization,
community membership, encrypted app indexes, and any compatibility-affecting
wire/schema change.
