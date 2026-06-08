# 079: Pastey Final Compatibility Pass

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready near freeze
**Blocked by:** 075, 076

## Why

Pastey was the first app-boundary proof. Before v0 freezes, it should be
checked against the final Jolt APIs so it remains a credible companion app and
regression canary.

## What to Build

In the Pastey repo:

- remove dev-only assumptions where practical;
- verify app-session request/approval still works;
- verify public paste publish/fetch still works;
- verify private/self-only paste still works;
- verify recipient private paste still works;
- verify optional pinning behavior;
- update setup docs to point at packaged Jolt if available.

## Acceptance Criteria

- [ ] Pastey works against current Jolt `dev`.
- [ ] Pastey does not use admin APIs for normal app behavior.
- [ ] Public paste workflow passes.
- [ ] Private paste workflow passes.
- [ ] Optional pinning behavior is documented.
- [ ] README/setup instructions are current.

## Non-Goals

- Turning Pastey into the flagship product.
- Adding social features to Pastey.
- App store integration.

## Notes

Pastey remains useful as a technical PoC even if Spoke becomes the human-facing
PoC.
