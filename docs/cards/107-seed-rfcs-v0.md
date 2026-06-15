# 107: Seed RFCs v0

**Type:** AFK after process
**Milestone:** Project Communication
**Status:** Ready after 106
**Blocked by:** 106

## Why

Once the RFC process exists, the first RFCs should capture the decisions that
are most likely to shape Jolt's compatibility surface.

This prevents the next sprint from implementing major identity, device,
community, or encryption semantics from scattered notes.

## What to Build

Create the initial RFC set from existing cards and docs:

- true multi-writer identity and per-device writer logs;
- device authorization and revocation;
- encrypted app indexes and private content device access;
- community identities, join policy, membership, and revocation;
- community-scoped app indexes and local search;
- app session scope across user identity, device, and app.

## Acceptance Criteria

- [ ] Each seed RFC has a clear status.
- [ ] Each seed RFC states motivation, model, compatibility impact, security
      considerations, and unresolved questions.
- [ ] Each seed RFC links to the relevant work cards.
- [ ] The RFC index links all seed RFCs.
- [ ] The website RFC page links to the RFC index when the website exists.
- [ ] No RFC claims implementation is complete unless the implementation has
      landed.

## Non-Goals

- Finalizing every open design question.
- Rewriting all historical docs as RFCs.
- Blocking small docs-only cards on RFC approval.

## Notes

The seed RFCs should be short enough to review but precise enough that future
agents do not need chat history to understand the decisions.
