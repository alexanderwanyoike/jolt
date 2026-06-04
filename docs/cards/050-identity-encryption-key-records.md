# 050: Identity Encryption Key Records

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Implemented in PR
**Blocked by:** 049

## Why

Alice can encrypt a paste for Bob only if she can discover and verify Bob's public encryption key. That key must be bound to Bob's `.jolt` identity by signed state.

## What to Build

Add signed identity metadata for encryption public keys.

The record should include:

- Identity owner.
- Encryption key ID.
- Key type.
- Public key bytes.
- Validity period or sequence.
- Signature by the identity signing key.

Resolution should allow:

```text
resolve bob.jolt encryption keys
verify keys belong to bob.jolt identity
select usable key for encryption
```

## Acceptance Criteria

- [x] Core type exists for identity encryption key records.
- [x] Records are signed by the identity signing key.
- [x] Resolver verifies key records against the target identity.
- [x] Daemon/API can expose verified public encryption keys for an identity.
- [x] Tests reject keys signed by the wrong identity.
- [x] Tests reject expired or unsupported keys if expiry/support is part of v0.

## Notes

Do not derive app access policy from this card. This card only binds public encryption keys to identities.

## Implementation Notes

- v0 identity encryption key records publish under `/.well-known/jolt/encryption-keys`.
- The core verifier accepts owner-signed records for the requested identity and returns current usable `x25519-hkdf-sha256` / `OKP` / `X25519` active keys.
- The server exposes `GET /api/v1/identities/{identity}/encryption-keys`, which resolves the reserved signed path, fetches the record content, verifies it, and returns verified public keys.

## Verification

- `cargo test -p jolt-core identity_encryption_key -- --nocapture`
- `cargo test -p jolt-server test_identity_encryption_keys_endpoint_returns_verified_record_keys -- --nocapture`
