# 049: Crypto Agility and Encrypted Object Envelope

**Type:** HITL  
**Milestone:** Private Sharing Foundations  
**Status:** Design in PR
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

- [x] A design note exists under `docs/`.
- [x] It defines `EncryptedObject` or equivalent envelope fields.
- [x] It includes `version`, `suite_id`, content encryption metadata, recipient wrap metadata, author, and signature.
- [x] It separates content encryption, key wrapping, signatures, and identity records.
- [x] It names a v0 implementation suite and a future post-quantum-hybrid suite direction.
- [x] It states that relays store ciphertext and do not participate in decryption.
- [ ] Human review confirms the direction before implementation.

## Notes

Apps should ask the daemon to encrypt/decrypt. Apps should not choose crypto algorithms directly.

## Design Note

Design proposal: [Encrypted Object Envelope](../16-encrypted-object-envelope.md).

## Verification

- Docs-only design card. No code tests required.
- Checked against primary standards:
  - RFC 9180 for HPKE.
  - RFC 7748 for X25519.
  - RFC 5869 for HKDF.
  - RFC 8439 for ChaCha20-Poly1305.
  - NIST FIPS 203 for ML-KEM.
