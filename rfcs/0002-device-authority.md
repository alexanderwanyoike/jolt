# Jolt Request for Comments 0002

## Device Authorization and Revocation

```text
Jolt Project                                                JOLT-RFC-0002
Request for Comments: 0002                                  August 2026
Category: Experimental
Status: Experimental Draft
Updates: none
Obsoletes: none
```

### Status of This Memo

This document specifies an experimental Jolt protocol and requests review of
its authority, encoding, revocation, and migration rules. It is not an IETF
publication. Distribution of this memo is unlimited.

The v1 record format described here is implemented, but this document remains
an experimental draft until the compatibility surface is accepted. Implementations
MUST NOT infer that implementation status freezes unresolved semantics.

### Abstract

This document defines how a durable Jolt identity authorizes independent device
writers without copying the identity root signing key to every device. It
specifies the device-authority record, its canonical signed encoding, authority
chain validation, device encryption-key advertisement, revocation cutoffs, and
the migration of legacy single-key identities.

Authorization permits a device to make statements for an identity. Revocation
prevents later statements from being accepted; it cannot erase plaintext or
secret key material already held by that device.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Primitive Encoding
6. Device Authority Record
7. Canonical Signature Encoding
8. Authority Chain Validation
9. Authorization Processing
10. Revocation Processing
11. Publication and Discovery
12. Error Conditions
13. Compatibility and Migration
14. Security Considerations
15. Privacy Considerations
16. IANA Considerations
17. Implementation Status
18. References
Appendix A. Worked State Transition

## 1. Introduction

A Jolt identity is a durable namespace. A laptop, phone, or server that writes
for that namespace is a replaceable device, not the identity itself. Sharing one
long-lived root secret among all devices would make individual revocation
impossible and turn every device compromise into a permanent identity
compromise.

This memo separates the identity root from device writers. The root key signs a
hash-chained authority log. Each authorization binds a stable device identifier
to an Ed25519 writer key, optional encryption keys, and generic capabilities.
Each revocation names a device and may bound the final device-writer sequence
that remains acceptable.

Device-writer operations are specified separately in JOLT-RFC-0003. Encrypted
object access using advertised device encryption keys is specified in
JOLT-RFC-0004.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are to be interpreted as described in BCP 14 [RFC2119] [RFC8174]
when they appear in all capitals.

`uint16be` and `uint64be` are unsigned integers in network byte order. `octets`
is a `uint64be` length followed by that many bytes. `string` is an `octets`
value containing valid UTF-8. An optional field begins with `0x00` for absent or
`0x01` followed by the encoded value for present.

## 3. Scope

This memo defines:

- root-authorized device signing keys;
- device encryption-key advertisements;
- authorization and revocation records;
- canonical bytes covered by Ed25519 signatures;
- hash-chain verification and authority materialization;
- accepted-through sequence cutoffs after revocation;
- the legacy-root migration device.

This memo does not define:

- device-writer log operations or conflict resolution;
- encrypted-object envelopes or content-key wrapping;
- app sessions or application permissions;
- social recovery, root rotation, remote wipe, or hardware key custody;
- application-specific device roles.

## 4. Terminology

**Root key**
: The Ed25519 key whose public key derives the Jolt identity identifier and
  whose private key authorizes v1 authority records.

**Device identifier**
: A non-empty UTF-8 string stable within one identity authority chain. It is a
  protocol identifier, not a human-facing identity.

**Device signing key**
: A 32-octet Ed25519 public key authorized to sign one device-writer log.

**Device encryption key**
: A 32-octet public key advertised for recipient content-key wrapping.

**Authority chain**
: A non-empty, ordered, root-signed sequence of device authority records.

**Accepted-through sequence**
: The greatest device-writer sequence accepted after the corresponding device
  has been revoked.

## 5. Primitive Encoding

The following helpers are used by Section 7:

```text
bytes(x)      = uint64be(len(x)) || x
string(s)     = bytes(UTF8(s))
list(xs, f)   = uint64be(count(xs)) || concat(f(x) for x in xs)
optional(x)   = 0x00, when absent
              = 0x01 || encode(x), when present
```

Receivers MUST reject lengths or counts exceeding local resource limits before
allocation. List order is significant and is covered by the signature.

## 6. Device Authority Record

The v1 logical record is:

```text
DeviceAuthorityRecord {
  body: DeviceAuthorityRecordBody,
  signature: bytes[64]
}

DeviceAuthorityRecordBody {
  record_type: "jolt.identity_authority_record",
  version: 1,
  root_public_key: bytes[32],
  identity: IdentityId,
  sequence: uint64,
  previous_record_hash: optional bytes[32],
  operation: AuthorizeDevice | RevokeDevice,
  issued_at: uint64
}
```

An authorization operation is:

```text
AuthorizeDevice {
  device_id: string,
  device_signing_public_key: bytes[32],
  device_encryption_keys: [DeviceEncryptionPublicKey],
  capabilities: [string],
  label: optional string,
  created_at: uint64
}

DeviceEncryptionPublicKey {
  key_id: string,
  suite_family: string,
  public_key: bytes[32],
  created_at: uint64
}
```

A revocation operation is:

```text
RevokeDevice {
  device_id: string,
  accepted_through_device_sequence: optional uint64,
  reason: optional string,
  created_at: uint64
}
```

`issued_at`, `created_at`, `label`, `reason`, and capability strings are signed
metadata. Time values MUST NOT independently create authority. Capability
strings unknown to a receiver MUST be retained but MUST NOT be interpreted as
granting a known privilege.

## 7. Canonical Signature Encoding

The signature payload begins with the literal domain separator including its
terminal NUL byte:

```text
"jolt:identity-authority-record:v1" || 0x00
```

The remainder is encoded in this exact order:

```text
string(record_type)
uint16be(version)
bytes(root_public_key)
string(identity)
uint64be(sequence)
optional(previous_record_hash)       ; present value is raw 32 bytes
operation
uint64be(issued_at)
```

`AuthorizeDevice` has discriminator `0x00` followed by:

```text
string(device_id)
bytes(device_signing_public_key)
uint64be(device_encryption_key_count)
  repeated for each key:
    string(key_id)
    string(suite_family)
    bytes(public_key)
    uint64be(created_at)
uint64be(capability_count)
  repeated string(capability)
optional(label)
uint64be(created_at)
```

`RevokeDevice` has discriminator `0x01` followed by:

```text
string(device_id)
optional(uint64be(accepted_through_device_sequence))
optional(reason)
uint64be(created_at)
```

The record signature is Ed25519 over the canonical body bytes. The record hash
is BLAKE3-256 over the same canonical body bytes; the signature is excluded.

## 8. Authority Chain Validation

A receiver MUST validate a candidate chain as one atomic value:

1. Reject an empty chain.
2. Validate every record type, version, key length, and device identifier.
3. Derive the identity from `root_public_key` and require it to equal the
   record identity and the requested identity.
4. Require every record to carry the same root public key.
5. Verify every signature with that root public key.
6. Require the genesis sequence to be zero and its previous hash to be absent.
7. For record index `i > 0`, require `sequence == i` and require the previous
   hash to equal the BLAKE3-256 body hash of record `i - 1`.
8. Apply operations in sequence order.
9. Reject a revocation naming a device not yet present in materialized state.

A receiver MUST NOT partially apply a chain that fails any step.

## 9. Authorization Processing

Applying `AuthorizeDevice` inserts or replaces the materialized entry for its
`device_id` and marks it active. The materialized entry contains the signing
key, encryption keys, capability strings, label, and authorization time.

Replacing an existing device identifier is a security-sensitive key change.
Writers SHOULD allocate a new device identifier for a new physical or logical
device. User interfaces MUST make replacement distinguishable from adding a
device.

The v1 authority chain is root-signed. Delegated admin-device authorization is
not part of this wire contract even if a capability string names an admin role.

## 10. Revocation Processing

Applying `RevokeDevice` marks the materialized device revoked and retains its
keys for historical verification.

For a revoked device:

- if `accepted_through_device_sequence` is present, entries with a sequence
  less than or equal to the cutoff remain eligible and later entries MUST be
  rejected;
- if the cutoff is absent, all device-writer entries MUST be rejected;
- new encrypted objects MUST NOT include that device's encryption keys;
- local app sessions bound to that device MUST cease authorizing future writes.

Revocation is prospective. It cannot invalidate content that other parties
already accepted under earlier authority, erase cached ciphertext, or make a
device forget decrypted plaintext.

## 11. Publication and Discovery

The current authority chain is published as signed Jolt state at:

```text
/.well-known/jolt/device-authority
```

The path binding establishes the content identifier of a serialized authority
chain. A resolver MUST verify both the signed path state and the authority chain
itself. A relay or provider supplying the bytes has no authority over the
device set.

Implementations MAY cache verified chains. They MUST compare candidate chain
sequences and MUST NOT replace a verified newer chain with a shorter stale
candidate solely because it arrived later.

## 12. Error Conditions

Implementations SHOULD expose distinct errors for at least:

- empty authority chain;
- invalid root, device signing, or device encryption key length;
- invalid signature or signature length;
- root/identity mismatch;
- unsupported record type or version;
- invalid genesis sequence or unexpected genesis previous hash;
- out-of-order sequence or broken previous hash;
- empty device identifier;
- revocation of an unknown device.

Malformed input MUST fail closed and MUST NOT mutate previously verified state.

## 13. Compatibility and Migration

An existing single-key identity migrates by authorizing a transitional device
identifier `dev_legacy_root`. That device may initially use the root Ed25519
key as its device writer key. The identity address does not change.

New implementations SHOULD generate separate device keys for subsequent
devices. A future record version may add root rotation or delegated admin
signatures; such a change requires a new version and compatibility rules.

## 14. Security Considerations

The root private key is the highest v1 authority and SHOULD be exposed as
little as possible. Compromise permits an attacker to authorize arbitrary
devices and rewrite future authority.

Receivers MUST verify chain continuity before using advertised device keys.
They MUST bind device-writer signatures to the exact key present in verified
authority state. They MUST enforce revocation cutoffs even when a stale provider
continues serving a longer device log.

`reason`, `label`, and wall-clock fields are informational. They MUST NOT
override signatures, sequence order, or hash continuity.

## 15. Privacy Considerations

Authority records reveal device identifiers, labels when present, key counts,
capability labels, authorization timing, and revocation timing. Publishers
SHOULD avoid personally identifying labels in public authority records.

Revoking a device publicly reveals that the device relationship changed. This
memo does not hide the number of devices attached to an identity.

## 16. IANA Considerations

This document requests no IANA actions. Record names and domain separators are
project-local experimental identifiers.

## 17. Implementation Status

The v1 authority record, canonical encoding, root signature verification,
chain validation, device authorization, revocation cutoff, device encryption
keys, legacy-root migration, admin routes, and focused negative tests are
implemented in `jolt-core` and `jolt-server`.

Known limitations include root-key-only authority, no root rotation, and a
transitional local device identifier for app sessions. Delivery was tracked by
cards 091 and 093. App-session invalidation is further specified by
JOLT-RFC-0007.

## 18. References

### 18.1 Normative References

- [RFC2119] Bradner, S., “Key words for use in RFCs to Indicate Requirement
  Levels,” March 1997.
- [RFC8174] Leiba, B., “Ambiguity of Uppercase vs Lowercase in RFC 2119 Key
  Words,” May 2017.
- [BLAKE3] O'Connor, J., et al., “The BLAKE3 Hashing Framework.”
- [Ed25519] Josefsson, S. and I. Liusvaara, “Edwards-Curve Digital Signature
  Algorithm (EdDSA),” RFC 8032.

### 18.2 Informative References

- JOLT-RFC-0001, “Signed Path Records and Resolution.”
- JOLT-RFC-0003, “Per-Device Writer Logs and Deterministic Merge.”
- Jolt card 093, “Device Authorization and Revocation v0.”

## Appendix A. Worked State Transition

Given records `A0`, `A1`, and `A2`:

```text
A0 authorize dev_laptop, signing key KL, sequence 0
A1 authorize dev_phone,  signing key KP, sequence 1, previous hash H(A0)
A2 revoke    dev_phone, accepted through 7, sequence 2, previous hash H(A1)
```

The materialized state contains an active laptop and a revoked phone. Writer
entries `dev_phone:0` through `dev_phone:7` remain historically eligible.
`dev_phone:8` and later are rejected regardless of which provider serves them.

