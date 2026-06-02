# 053: Pastey Private Paste v0

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Ready after 052  
**Blocked by:** 047, 052

## Why

Pastey is the smallest app that can prove Jolt's private sharing path: Alice publishes an encrypted paste for Bob, Bob fetches it through the network, and only Bob can decrypt it.

## What to Build

Update Pastey with a private paste flow:

- Choose public or private paste.
- Enter recipient `.jolt` identities.
- Resolve recipient encryption keys.
- Publish encrypted paste under `/pastes/*`.
- Open encrypted paste.
- Decrypt through the daemon when authorized.
- Show clear unauthorized/decryption failure states.

## Acceptance Criteria

- [ ] Alice can create a private paste for Bob.
- [ ] Bob can open Alice's `.jolt` paste address and read it.
- [ ] Carol can fetch the encrypted bytes but cannot decrypt them.
- [ ] Relay/cache behavior works with ciphertext.
- [ ] Pastey clearly labels public versus encrypted pastes.
- [ ] Tests or a documented local process demo cover Alice, Bob, and unauthorized Carol.

## Notes

This is the first real permissioned-access product proof. Keep scope small and paste-focused.
