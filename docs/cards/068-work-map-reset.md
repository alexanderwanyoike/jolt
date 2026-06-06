# 068: Work Map Reset

**Type:** AFK  
**Milestone:** Planning Hygiene  
**Status:** Implemented in PR  
**Blocked by:** 067

## Why

After the Console, private sharing, and relay-operator slices landed, the card
index still described several completed items as current or next work. That made
the project map harder to trust when choosing the next card.

## What to Build

Refresh the card index so it clearly separates:

- What Jolt can already do.
- Which app/private-sharing cards are complete.
- Which Console-native daemon UX cards are complete.
- Which tracks still need human direction or another implementation slice.

## Acceptance Criteria

- [x] `docs/cards/README.md` no longer presents completed app-boundary work as
  the current next sprint.
- [x] Card 052 status in the index matches the card file.
- [x] The index lists the remaining decision tracks clearly enough for a fresh
  session to continue without relying on chat history.

## Verification Notes

- Docs-only change; no code tests required.
- Verified by reading the updated card index.

## Notes

This card intentionally does not choose the next product direction. It makes the
choice explicit so the next PR can focus on one track.
