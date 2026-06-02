# 051: Encrypted Object Implementation v0

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Ready after 049 and 050  
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

- [ ] Core encrypted object types exist.
- [ ] Encrypting for one recipient produces ciphertext and one wrapped key.
- [ ] Encrypting for multiple recipients produces one ciphertext and multiple wrapped keys.
- [ ] Recipient can unwrap and decrypt.
- [ ] Non-recipient cannot decrypt.
- [ ] Tampered envelope or ciphertext fails verification/decryption.
- [ ] Encrypted content can be stored/fetched/pinned like other content.

## Notes

The network layer should carry encrypted bytes without understanding Pastey, friends, groups, or access intent.
