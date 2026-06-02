# 052: Daemon Encrypt/Decrypt API

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Ready after 044 and 051  
**Blocked by:** 044, 051

## Why

Apps should not hold long-term private keys or implement crypto policy. The daemon owns local identity keys and should encrypt/decrypt only when an app session has the right capability.

## What to Build

Add app-facing daemon APIs for encryption and decryption:

- Encrypt plaintext for recipient `.jolt` identities.
- Publish encrypted object under an approved path.
- Fetch encrypted object.
- Decrypt encrypted object if the local identity is a recipient and the session has permission.

Capability checks should cover:

- Which identity is used.
- Which path prefix is allowed.
- Whether decrypt is allowed.
- Whether publish encrypted content is allowed.

## Acceptance Criteria

- [ ] App API can encrypt for verified recipient identities.
- [ ] App API can publish encrypted content under an approved path.
- [ ] App API can decrypt only when the local daemon has a matching private key.
- [ ] App session without decrypt capability cannot decrypt.
- [ ] App session outside the path scope cannot publish/decrypt that path.
- [ ] Tests cover allowed decrypt, denied decrypt, and non-recipient decrypt failure.

## Notes

Private access control belongs in the daemon/app boundary. Relays should still only see ciphertext.
