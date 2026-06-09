# 090: v0 Demo and Launch Writeup

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Ready after 085/087/088/089
**Blocked by:** 080, 085, 087, 088, 089

## Why

After Jolt, Pastey, Spoke, and a bootstrap relay are installable, the project
needs a simple public demo story and an honest write-up. The goal is to learn
whether people understand or want the thing, not to keep building indefinitely.

## What to Do

- Write a concise Medium/HN-style post explaining Jolt as a platformless
  content distribution network.
- Link install instructions for:
  - Jolt Console + `jolt` CLI;
  - Pastey;
  - Spoke.
- Explain the bootstrap relay used for the demo and what it does.
- Explain relay pinning policy honestly: discovery open, pinning allowlisted for
  v0.
- Show the simplest demo:
  1. install Jolt;
  2. install Spoke;
  3. approve Spoke in Console;
  4. publish a post;
  5. another identity discovers/reads/replies;
  6. optionally use Pastey as a smaller technical proof.
- State limitations and known rough edges.
- Decide whether to continue, pause, or bin the project after feedback.

## Acceptance Criteria

- [ ] Draft explains what Jolt is in non-protocol-first language.
- [ ] Draft includes a short architecture/model section.
- [ ] Draft links working install commands.
- [ ] Draft includes Spoke as the primary demo.
- [ ] Draft includes Pastey as a companion proof, not the main pitch.
- [ ] Draft states limitations: rough UX, eventual consistency, identity names,
      app ecosystem, relay trust/availability, and Linux-first packaging.
- [ ] HN title/options are drafted.
- [ ] Continue/pause/bin decision criteria are restated.

## Non-Goals

- Marketing site.
- Launching an app store.
- Claiming production security maturity.
- Hiding limitations.

## Notes

This card supersedes the launch/write-up portion of card 081 with the concrete
artifact list needed after the distribution and relay cards land.
