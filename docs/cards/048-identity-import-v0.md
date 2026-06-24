# 048: Identity Import v0

**Type:** HITL
**Milestone:** Identity Management
**Status:** Designed in PR
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

- [x] A design note or card update defines the v0 identity import/export format.
- [x] Export is explicitly admin-only and unavailable to normal app sessions.
- [x] Risks are documented: compromise, no per-device revocation, concurrent update-log conflicts.
- [x] Future delegated device-key model is sketched but not implemented.
- [ ] Human review confirms the direction before implementation.

## Design

See [Identity Import and Export v0](../18-identity-import-export.md).

Key decisions:

- v0 import/export is an admin/Console trust-class action, never a normal app
  session capability.
- The export file is an encrypted recovery bundle, not an app data export.
- The bundle must include the root identity signing secret and local identity
  encryption private keys needed to decrypt existing private objects.
- Export supports an optional SSH-key-style passphrase using Argon2id and
  XChaCha20-Poly1305. Without a passphrase, the export file itself is enough to
  act as the identity.
- v0 shared-key import is positioned as recovery or deliberate device move, not
  safe seamless multi-device collaboration.
- Delegated device keys are the future model for scoped per-device revocation,
  but are not implemented in this card.

## Verification Notes

- Docs-only design. No code tests were run.
- Verified by reading the updated design note and card index.

## Notes

This card is intentionally cautious. Exporting private keys is dangerous and should never be exposed through normal app permissions.
