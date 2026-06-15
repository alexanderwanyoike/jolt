# 093: Device Authorization and Revocation v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 091  
**Blocked by:** 091

## Why

Users should not have to copy a root user-identity private key to every machine.
Each device needs its own key material and revocable authority to write for a
user identity.

This gives Jolt a credible answer for new devices, lost devices, and removing a
device that should no longer publish as the user.

## What to Build

Implement device authorization records for a user identity:

- generate or register a local device identity;
- authorize that device as a writer for a user identity;
- publish the authorization as signed identity state;
- resolve authorized devices for an identity;
- revoke a device and publish that revocation;
- reject writes from revoked or unknown devices.

## Acceptance Criteria

- [ ] A user identity can authorize at least one device writer.
- [ ] A user identity can authorize a second device without sharing the first
      device's private key.
- [ ] Device authorization records are signed and verifiable.
- [ ] Device revocation records are signed and verifiable.
- [ ] Revoked devices cannot produce accepted new identity state after the
      revocation point.
- [ ] Existing single-device identities have a clear transitional path.
- [ ] Tests cover authorized write, unauthorized write, and revoked-device
      rejection.

## Non-Goals

- Full account recovery.
- Hardware security module support.
- Remote wipe guarantees for data already cached on a revoked device.
- Browser sessions.

## Notes

Revocation prevents future accepted writes and future key wrapping. It cannot
make a device forget content or plaintext it already cached.

