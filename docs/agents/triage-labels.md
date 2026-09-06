# Triage metadata

Jolt's private work cards use `Type` and `Status` metadata instead of external issue labels.

## Type

| Meaning | Card metadata |
|---|---|
| An agent can implement the card without further product decisions | `Type: AFK` |
| The card requires a human decision or interactive review | `Type: HITL` |

## Status

| Meaning | Card metadata |
|---|---|
| Reviewed and ready when dependencies are satisfied | `Status: Ready` |
| Waiting for missing information or an external dependency | `Status: Waiting for information` |
| Deliberately not proceeding | `Status: Won't fix` |
| Draft exists but has not been reviewed | `Status: Needs review` |

## Rules

- A card is agent-ready only when it has both `Type: AFK` and `Status: Ready`.
- `Status: Ready` does not override entries in `Blocked by`.
- Use `Type: HITL` whenever an unresolved product or architecture decision could materially change the implementation.
- Do not invent additional metadata values without documenting them here first.
