# 050: Identity Encryption Key Records

**Type:** AFK  
**Milestone:** Private Sharing Foundations  
**Status:** Ready after 049  
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

- [ ] Core type exists for identity encryption key records.
- [ ] Records are signed by the identity signing key.
- [ ] Resolver verifies key records against the target identity.
- [ ] Daemon/API can expose verified public encryption keys for an identity.
- [ ] Tests reject keys signed by the wrong identity.
- [ ] Tests reject expired or unsupported keys if expiry/support is part of v0.

## Notes

Do not derive app access policy from this card. This card only binds public encryption keys to identities.
