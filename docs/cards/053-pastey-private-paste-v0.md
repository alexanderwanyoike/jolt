# 053: Pastey Private Paste v0

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Implemented in Pastey PR
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

- [x] Alice can create a private paste for Bob.
- [x] Bob can open Alice's `.jolt` paste address and read it.
- [x] Carol can fetch the encrypted bytes but cannot decrypt them.
- [x] Relay/cache behavior works with ciphertext.
- [x] Pastey clearly labels public versus encrypted pastes.
- [x] Tests or a documented local process demo cover Alice, Bob, and unauthorized Carol.

## Verification Notes

- Pastey PR: `https://github.com/alexanderwanyoike/pastey/pull/2`.
- Pastey now requests `encrypt:/pastes/*`, `decrypt:/pastes/*`, and `publish:encrypted:/pastes/*`.
- Pastey has explicit Public/Encrypted modes for composing and opening pastes.
- Encrypted open first fetches ciphertext, then asks the daemon to decrypt; unauthorized readers see a ciphertext-fetched/decrypt-failed state.
- README documents the Alice/Bob/Carol local process demo.
- Verified in Pastey with `npm test` and `npm run build`.
- Checked desktop and mobile layout with Chrome headless screenshots.

## Notes

This is the first real permissioned-access product proof. Keep scope small and paste-focused.
