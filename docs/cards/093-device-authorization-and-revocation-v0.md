# 093: Device Authorization and Revocation v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** In review
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

- [x] A user identity can authorize at least one device writer.
- [x] A user identity can authorize a second device without sharing the first
      device's private key.
- [x] Device authorization records are signed and verifiable.
- [x] Device revocation records are signed and verifiable.
- [x] Revoked devices cannot produce accepted new identity state after the
      revocation point.
- [x] Existing single-device identities have a clear transitional path.
- [x] Tests cover authorized write, unauthorized write, and revoked-device
      rejection.

## Non-Goals

- Full account recovery.
- Hardware security module support.
- Remote wipe guarantees for data already cached on a revoked device.
- Browser sessions.

## Notes

Revocation prevents future accepted writes and future key wrapping. It cannot
make a device forget content or plaintext it already cached.

## Implementation Notes

- Added signed identity authority records to `jolt-core`, with root-signed
  authorization and revocation operations.
- Added deterministic authority-chain verification, materialized authorized
  device state, and `device_can_write(device_id, sequence)` for revoked-device
  cutoff checks.
- Added negative authority-chain tests for non-root signatures, broken previous
  hashes, out-of-order sequences, unknown-device revocation, revoked devices
  without accepted-through cutoffs, and device encryption key preservation.
- Added a transitional `dev_legacy_root` device record for the current
  single-key identity model.
- Added admin endpoints for listing the local authority chain, authorizing a
  generated local device, and revoking a device.
- Device authority mutations now validate the candidate chain before committing
  it to local state, so rejected revocations do not poison the in-memory chain.
- Added device-authority API regression tests for idempotent local authority
  bootstrap/listing and continuing with valid mutations after a rejected
  revocation.
- The daemon signs authority records internally; the HTTP server never receives
  private key material.
- The verified authority chain is published under
  `/.well-known/jolt/device-authority` as signed identity state.

## Verification

- `cargo test -p jolt-core --test identity_authority -- --nocapture`
- `cargo test -p jolt-server test_admin_device_authority_can_authorize_and_revoke_local_device --test api_integration -- --nocapture`
- `cargo test -p jolt-server test_admin_device_authority_list_is_idempotent --test api_integration -- --nocapture`
- `cargo test -p jolt-server test_admin_device_authority_rejects_unknown_device_revocation --test api_integration -- --nocapture`
- `cargo test -p jolt-server test_admin_device_authority_can_continue_after_rejected_revocation --test api_integration -- --nocapture`
- `cargo test -p jolt-server device_authority --test api_integration -- --nocapture`
- `cargo test -p jolt-server identity --test api_integration -- --nocapture`
- `./scripts/test-local.sh`
