# 010: Crypto Agility Spike

**Type:** HITL  
**Milestone:** M6 preparation  
**Status:** Later  
**Blocked by:** None

## Why

Jolt's access-control docs say encryption should be crypto-agile and eventually post-quantum-aware. That decision should be made deliberately before implementing private content.

## What to Decide

Produce a short design note answering:

- Which algorithms are used for v0 private content?
- How are encryption scheme identifiers encoded in manifests?
- How can content/key envelopes migrate to hybrid or post-quantum schemes?
- Which parts of identity remain Ed25519 for now?
- What must be abstracted so future crypto migrations do not rewrite the protocol?

## Acceptance Criteria

- [ ] A design note exists under `docs/`.
- [ ] It separates signatures, key exchange, content encryption, and key envelopes.
- [ ] It defines a manifest-level algorithm/version field.
- [ ] It identifies what is v0 implementation vs future migration.
- [ ] Human review confirms the direction before M6 implementation starts.

## Notes

This is intentionally HITL. Do not implement crypto based only on this card.

