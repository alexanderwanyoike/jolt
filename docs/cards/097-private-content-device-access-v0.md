# 097: Private Content Device Access v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** In progress
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

- [x] New private writes can be encrypted for the current authorized device set.
- [x] Private app indexes can be encrypted for the current authorized device
      set.
- [x] A newly authorized device can decrypt future private content when included
      in the key wrap set.
- [x] Historical private content is not assumed to be readable unless rewrapped
      or already wrapped for that device.
- [x] Revoked devices are excluded from future key wrapping.
- [x] Apps can detect and communicate that old private content needs rewrap.
- [x] Tests cover new-device future decrypt, historical no-access, rewrap, and
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

## Implementation Notes

- Newly authorized generated devices now carry a device encryption public key in
  the signed device-authority record and admin device-authority response.
- App encrypted publish and append paths include the daemon's local identity
  encryption key plus active authorized device encryption keys for self-private
  writes.
- Future encrypted writes filter device-authority keys to active devices, so
  revoked devices are not included in new key wrap sets.
- `POST /app/v1/encrypted/open` now returns `access_status`:
  `available`, `needs_rewrap`, or `not_accessible`.
- `POST /app/v1/encrypted/rewrap` decrypts a readable historical encrypted
  object and republishes it at the same path for the current active authorized
  device set. It requires `decrypt:`, `encrypt:`, and `publish:encrypted:`
  capability for the path.
- Generated device private keys are still daemon-internal in this slice. The
  integration tests prove future encrypted objects include active authorized
  device key wraps; the envelope HPKE path remains the decryption mechanism once
  a device has its private key.

## Verification

- Red: `cargo test -p jolt-server test_app_encrypts_self_private_content_for_active_authorized_devices --test api_integration -- --nocapture`
  failed with `recipient_count` 1 before encrypted app writes consulted active
  device authority keys.
- Green: `cargo test -p jolt-server test_app_can_encrypt_append_and_enumerate_records_by_prefix --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server test_app_encrypts_self_private_content_for_active_authorized_devices --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server test_app_encrypted_publish_excludes_revoked_authorized_devices --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server test_admin_device_authority_can_authorize_and_revoke_local_device --test api_integration -- --nocapture`
- Red: `cargo test -p jolt-server test_app_open_reports_historical_private_content_needs_rewrap --test api_integration -- --nocapture`
  failed while encrypted open responses had no `access_status`.
- Red: `cargo test -p jolt-server test_app_can_rewrap_historical_private_content_for_authorized_devices --test api_integration -- --nocapture`
  failed with `404` before `/app/v1/encrypted/rewrap` existed.
- Green: `cargo test -p jolt-server test_app_open_reports_historical_private_content_needs_rewrap --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server test_app_can_rewrap_historical_private_content_for_authorized_devices --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server encrypt --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server device_authority --test api_integration -- --nocapture`
- Green: `cargo test -p jolt-server --test api_integration -- --nocapture`
- Green: `./scripts/test-local.sh`
