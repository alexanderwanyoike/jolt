# 049: Crypto Agility and Encrypted Object Envelope

**Type:** HITL  
**Milestone:** Private Sharing Foundations  
**Status:** Ready  
**Blocked by:** 042

## Why

Private Pastey requires encryption, but Jolt should not hardcode one quick classical-only scheme. Encrypted objects need explicit suite identifiers, key types, recipient wrapping metadata, and a path toward post-quantum-hybrid encryption.

## What to Decide

Write a design note covering:

- Encrypted object envelope structure.
- Algorithm suite identifiers.
- Content encryption algorithm for v0.
- Recipient key wrapping algorithm for v0.
- Hybrid classical + post-quantum KEM direction.
- How identity encryption keys are published and verified.
- How apps request encryption/decryption through the daemon without selecting algorithms directly.

## Acceptance Criteria

- [ ] A design note exists under `docs/`.
- [ ] It defines `EncryptedObject` or equivalent envelope fields.
- [ ] It includes `version`, `suite_id`, content encryption metadata, recipient wrap metadata, author, and signature.
- [ ] It separates content encryption, key wrapping, signatures, and identity records.
- [ ] It names a v0 implementation suite and a future post-quantum-hybrid suite direction.
- [ ] It states that relays store ciphertext and do not participate in decryption.
- [ ] Human review confirms the direction before implementation.

## Notes

Apps should ask the daemon to encrypt/decrypt. Apps should not choose crypto algorithms directly.
