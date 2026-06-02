# 048: Identity Import v0

**Type:** HITL  
**Milestone:** Identity Management  
**Status:** Ready for design  
**Blocked by:** None

## Why

Users need to carry the same `.jolt` identity across devices. The simplest v0 model is importing the same identity private key onto another daemon, while clearly documenting the risks.

## What to Decide

Define the v0 import/export story:

- Export format.
- Import command.
- Whether export is CLI/admin-only.
- How to protect exported key material.
- How to warn about shared-key risks.
- How this evolves toward delegated device keys.

## Acceptance Criteria

- [ ] A design note or card update defines the v0 identity import/export format.
- [ ] Export is explicitly admin-only and unavailable to normal app sessions.
- [ ] Risks are documented: compromise, no per-device revocation, concurrent update-log conflicts.
- [ ] Future delegated device-key model is sketched but not implemented.
- [ ] Human review confirms the direction before implementation.

## Notes

This card is intentionally cautious. Exporting private keys is dangerous and should never be exposed through normal app permissions.
