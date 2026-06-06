# 080: v0 Freeze and Bugfix Window

**Type:** HITL  
**Milestone:** v0 Endgame  
**Status:** Active
**Blocked by:** None for Jolt README/freeze docs; Pastey and Spoke setup docs
still need their own final compatibility pass.

## Why

After Spoke and Pastey can be used against packaged Jolt, the project needs a
hard stop. Continuing to add features would hide the real question: does anyone
care enough to use this?

## Freeze Rule

After this card starts:

- no new protocol features;
- no new Console surfaces except bug fixes;
- no new app capabilities except bug fixes;
- no app store/catalog work;
- no relay structured logs/metrics;
- bug fixes, docs, setup polish, and demo fixes only.

## What to Do

- Run the full Jolt local suite.
- Run the Pastey human/demo path.
- Run the Spoke human/demo path.
- Fix blocking bugs.
- Write setup docs.
- Write demo docs.
- Record known limitations honestly.

## Acceptance Criteria

- [x] Jolt README documents current setup, v0 status, known limitations, and
      distribution as the next product step.
- [ ] Pastey setup/demo docs are current.
- [ ] Spoke setup/demo docs are current.
- [x] Known limitations are documented.
- [ ] No v0-blocking bugs remain.
- [ ] Full local test suite passes.
- [ ] Human demo has been run end to end.

## Non-Goals

- Any feature not required to run the v0 demos.

## Notes

This card is intentionally restrictive.
