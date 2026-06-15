# 097: Private Content Device Access v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 093 and 096  
**Blocked by:** 093, 096

## Why

If a user authorizes a new device, public app data can follow through signed
indexes and CID fetches. Private app data needs an extra rule: the new device
must have a valid decryption path. That applies to both encrypted content bodies
and encrypted app indexes, because a private paste list can be sensitive even
before the paste body is opened.

Jolt already has encrypted object envelopes and daemon-owned encrypt/decrypt
APIs. Multi-device identity needs to extend that model so private content can
follow authorized devices without exposing long-lived identity private keys.

## What to Build

Add the private-data side of device authorization:

- publish encryption keys for authorized devices;
- wrap new private content keys for currently authorized devices where
  appropriate;
- wrap private app index keys for currently authorized devices where
  appropriate;
- define what newly authorized devices can decrypt by default;
- add an explicit rewrap path for historical private content;
- stop wrapping future content to revoked devices;
- expose enough status for apps to explain "available", "needs rewrap", or
  "not accessible from this device."

## Acceptance Criteria

- [ ] New private writes can be encrypted for the current authorized device set.
- [ ] Private app indexes can be encrypted for the current authorized device
      set.
- [ ] A newly authorized device can decrypt future private content when included
      in the key wrap set.
- [ ] Historical private content is not assumed to be readable unless rewrapped
      or already wrapped for that device.
- [ ] Revoked devices are excluded from future key wrapping.
- [ ] Apps can detect and communicate that old private content needs rewrap.
- [ ] Tests cover new-device future decrypt, historical no-access, rewrap, and
      revoked-device exclusion.

## Non-Goals

- Remote deletion of already decrypted content.
- Social recovery.
- Group access-control policy beyond explicit recipient/device wrapping.
- Production-grade key escrow.

## Notes

The honest rule is:

```text
revocation protects future writes and future encryption access;
it cannot claw back plaintext already held by a device.
```
