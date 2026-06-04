# 051: Encrypted Object Implementation v0

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Implemented in PR
**Blocked by:** 049, 050

## Why

Jolt needs a generic encrypted content object that can be fetched, pinned, cached, and verified like public content while keeping plaintext unreadable to relays and unauthorized peers.

## What to Build

Implement encrypted object support according to [049](049-crypto-agility-encrypted-object-envelope.md):

- Generate a random content key.
- Encrypt plaintext once with that content key.
- Wrap the content key for each recipient identity.
- Store encrypted content as content-addressed bytes.
- Store/sign the encrypted object envelope.
- Verify envelope signature and ciphertext integrity.

## Acceptance Criteria

- [x] Core encrypted object types exist.
- [x] Encrypting for one recipient produces ciphertext and one wrapped key.
- [x] Encrypting for multiple recipients produces one ciphertext and multiple wrapped keys.
- [x] Recipient can unwrap and decrypt.
- [x] Non-recipient cannot decrypt.
- [x] Tampered envelope or ciphertext fails verification/decryption.
- [x] Encrypted content can be stored/fetched/pinned like other content.

## Notes

The network layer should carry encrypted bytes without understanding Pastey, friends, groups, or access intent.

## Implementation Notes

- v0 encrypted objects use suite ID `jolt.enc.v1.x25519-hkdf-sha256-chacha20poly1305.ed25519`.
- Plaintext is encrypted once with ChaCha20-Poly1305 and a random content key.
- Recipient wraps use HPKE Base mode with X25519, HKDF-SHA256, and ChaCha20-Poly1305.
- The author signs canonical envelope bytes with the identity signing key.
- Serialized envelopes are ordinary content-addressed bytes; the store/network layer does not need app-specific knowledge.

## Verification

- `cargo test -p jolt-core encrypted_object -- --nocapture`
- `cargo test -p jolt-store encrypted_object_bytes_can_be_cached_pinned_and_read_back -- --nocapture`
