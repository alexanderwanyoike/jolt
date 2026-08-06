# Jolt Request for Comments 0004

## Encrypted Objects and Private Device Access

```text
Jolt Project                                                JOLT-RFC-0004
Request for Comments: 0004                                  August 2026
Category: Experimental
Status: Experimental Draft
Updates: none
Obsoletes: none
```

### Status of This Memo

This document specifies an experimental Jolt encrypted-object and multi-device
access protocol. It is not an IETF publication. Distribution of this memo is
unlimited.

The v1 envelope and daemon operations are implemented. The memo remains a draft
while device-key custody and historical rewrap behavior receive compatibility
review.

### Abstract

This document defines a signed, content-addressable encrypted-object envelope.
Object bytes are encrypted once with ChaCha20-Poly1305. The content key is
wrapped independently to X25519 recipient keys using HPKE, allowing the same
ciphertext to be cached by untrusted peers and opened by authorized identities
or devices.

The document also defines how current device authority affects future wraps,
how encrypted app indexes follow an identity, how newly authorized devices gain
future access, how historical objects report missing access, and how an
authorized device republishes a rewrapped envelope without pretending that
revocation can erase previously decrypted plaintext.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Cryptographic Suite
6. Identity Encryption-Key Records
7. Encrypted Object Envelope
8. Canonical Signature and AAD Encodings
9. Encryption Procedure
10. Decryption Procedure
11. Authorized Device Recipient Selection
12. Encrypted App Indexes
13. Historical Access and Rewrap
14. Revocation Semantics
15. Error Conditions
16. Compatibility and Versioning
17. Security Considerations
18. Privacy Considerations
19. IANA Considerations
20. Implementation Status
21. References
Appendix A. Access-State Examples

## 1. Introduction

Content addressing provides integrity, not secrecy. Transport encryption
protects a connection, not bytes stored later by a cache or relay. Private Jolt
objects therefore require end-to-end encryption whose trust does not depend on
the provider that stores or serves the ciphertext.

Jolt uses a hybrid envelope. A random content-encryption key encrypts the
plaintext once. HPKE wraps that key separately to each eligible recipient key.
The author signs the complete envelope body, and the serialized envelope is
itself named by a CID.

An app index can be sensitive even when every referenced body is encrypted. The
same envelope therefore applies both to private bodies and to private indexes
bound under application-agnostic Jolt paths.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as BCP 14 [RFC2119] [RFC8174]. Primitive binary
encodings are those in JOLT-RFC-0002 Section 5.

## 3. Scope

This memo defines:

- identity encryption-key advertisement;
- the v1 encrypted-object suite and envelope fields;
- canonical author signatures and authenticated-data construction;
- per-recipient HPKE content-key wrapping;
- current-authorized-device recipient selection;
- encrypted identity/app indexes;
- `available`, `needs_rewrap`, and `not_accessible` outcomes;
- rewrap and prospective revocation behavior.

This memo does not define:

- group keys, social recovery, escrow, or DRM;
- hiding path, CID, size, timing, or recipient-count metadata;
- remote deletion of ciphertext or plaintext;
- automatic access to all historical objects for a new device;
- application schemas carried by the plaintext.

## 4. Terminology

**Content-encryption key (CEK)**
: A fresh 32-octet symmetric key generated for one encrypted object.

**Recipient wrap**
: An HPKE encapsulation and ciphertext carrying the CEK to one recipient key.

**Identity encryption key**
: An X25519 public key authorized by signed identity state.

**Device encryption key**
: An X25519 public key attached to an active JOLT-RFC-0002 device.

**Rewrap**
: Decrypting a readable historical object and publishing a new envelope whose
  recipient set reflects current authority.

## 5. Cryptographic Suite

The only v1 suite identifier is:

```text
jolt.enc.v1.x25519-hkdf-sha256-chacha20poly1305.ed25519
```

It fixes:

- author signatures: Ed25519;
- content encryption: ChaCha20-Poly1305 with a 32-octet key and 12-octet nonce;
- key wrapping: HPKE base mode with X25519-HKDF-SHA256 and
  ChaCha20-Poly1305;
- content and envelope identifiers: the CID construction in JOLT-RFC-0001.

Implementations MUST reject an unsupported suite rather than substituting an
algorithm under the v1 suite identifier.

## 6. Identity Encryption-Key Records

The well-known path for identity encryption keys is:

```text
/.well-known/jolt/encryption-keys
```

The signed logical record is:

```text
IdentityEncryptionKeyRecordBody {
  record_type: "jolt.identity_encryption_keys",
  version: 1,
  owner_public_key: bytes[32],
  identity: IdentityId,
  keys: [IdentityEncryptionKey],
  sequence: uint64,
  issued_at: uint64
}

IdentityEncryptionKey {
  key_id: string,
  suite_family: "x25519-hkdf-sha256",
  key_type: "OKP",
  curve: "X25519",
  public_key: bytes[32],
  created_at: uint64,
  not_before: uint64,
  expires_at: optional uint64,
  status: "active"
}
```

The body signature is Ed25519 by the identity owner key. Its canonical payload
begins with `"jolt:identity-encryption-key-record:v1" || 0x00`, then encodes
record type, version, owner key, identity, ordered key list, sequence, and
issued time using the primitives from JOLT-RFC-0002.

A receiver MUST derive the identity from `owner_public_key`, verify the
signature, and filter out keys with an unsupported family/type/curve, inactive
status, future `not_before`, or expired `expires_at`. At least one usable key is
required for recipient encryption.

## 7. Encrypted Object Envelope

The v1 envelope is serialized as UTF-8 JSON but signed using the binary
canonical encoding in Section 8.

```text
EncryptedObjectEnvelope {
  body: EncryptedObjectBody,
  signature: bytes[64]
}

EncryptedObjectBody {
  record_type: "jolt.encrypted_object",
  version: 1,
  suite_id: string,
  author: {
    identity: IdentityId,
    public_key: bytes[32]
  },
  plaintext: {
    media_type: string,
    schema: optional string,
    declared_size: uint64
  },
  content_encryption: {
    alg: "CHACHA20-POLY1305",
    nonce: bytes[12]
  },
  ciphertext: bytes,
  recipients: [RecipientWrap],
  created_at: uint64
}

RecipientWrap {
  recipient_identity: IdentityId,
  recipient_key_id: string,
  wrap_alg: "HPKE-BASE-X25519-HKDF-SHA256-CHACHA20POLY1305",
  encapped_key: bytes,
  wrapped_key: bytes
}
```

The author identity MUST be derived from the author public key. Recipient list
order is signed and therefore significant to the envelope bytes, though it
does not grant precedence among recipients.

## 8. Canonical Signature and AAD Encodings

### 8.1 Envelope Signature

The signed payload begins with:

```text
"jolt:encrypted-object-envelope:v1" || 0x00
```

It then encodes, in order:

```text
string(record_type)
uint16be(version)
string(suite_id)
string(author.identity)
bytes(author.public_key)
string(plaintext.media_type)
optional(plaintext.schema)
uint64be(plaintext.declared_size)
string(content_encryption.alg)
bytes(content_encryption.nonce)
bytes(ciphertext)
uint64be(recipient_count)
  repeated:
    string(recipient_identity)
    string(recipient_key_id)
    string(wrap_alg)
    bytes(encapped_key)
    bytes(wrapped_key)
uint64be(created_at)
```

### 8.2 Content AAD

Content encryption binds the author identity, plaintext metadata, content
algorithm, and creation time under the domain:

```text
"jolt:encrypted-object-content-aad:v1" || 0x00
```

The AAD MUST be constructed identically during encryption and decryption. A
metadata change therefore causes authentication failure even before author
signature policy is considered. Content AAD does not include the nonce. The
nonce is nevertheless covered by the author's signature over the canonical
envelope body and is supplied directly to the AEAD operation.

### 8.3 Recipient-Wrap AAD and HPKE Info

Each recipient wrap binds the author identity, recipient identity, and
recipient key identifier under:

```text
"jolt:recipient-content-key-wrap-aad:v1" || 0x00
```

The HPKE `info` value is the literal byte string:

```text
jolt:hpke-content-key-wrap:v1
```

## 9. Encryption Procedure

To encrypt an object, a sender MUST:

1. Validate that the author public key derives the author identity.
2. Select explicit recipient keys plus any current device keys required by
   Section 11.
3. Generate a fresh uniformly random 32-octet CEK and 12-octet nonce.
4. Construct plaintext metadata and content AAD.
5. Encrypt the plaintext once with ChaCha20-Poly1305.
6. For each recipient, validate the X25519 key and create one HPKE base-mode
   wrap of the CEK using recipient-wrap AAD and the fixed HPKE info.
7. Construct the envelope body in deterministic recipient order chosen by the
   implementation.
8. Sign the canonical body bytes with the author Ed25519 key.
9. Serialize the full envelope and compute its Jolt CID.

A sender MUST NOT reuse a CEK/nonce pair across objects.

## 10. Decryption Procedure

To decrypt, a receiver MUST:

1. parse the envelope and validate type, version, suite, key lengths, and nonce;
2. verify that the author public key derives the claimed identity;
3. verify the Ed25519 signature over the canonical body;
4. find a wrap whose recipient identity and key ID match the local private key;
5. open the HPKE wrap to recover a 32-octet CEK;
6. reconstruct content AAD and authenticate/decrypt the ciphertext;
7. require the plaintext length to agree with declared size subject to local
   resource policy.

Failure at any step MUST return no plaintext.

## 11. Authorized Device Recipient Selection

For a private write owned by the local identity, the daemon resolves verified
JOLT-RFC-0002 authority and forms the device recipient set from encryption keys
attached to active authorized devices. Revoked devices MUST be excluded.

The daemon also includes the local identity encryption key required for the
current device to retain access. Duplicate `(identity, key_id)` recipients
SHOULD be removed before encryption.

A newly authorized device is eligible only for envelopes created after its key
enters verified authority. Authorization does not rewrite historical CIDs.

## 12. Encrypted App Indexes

An encrypted app index is an ordinary encrypted object whose CID is referenced
by a signed singleton or append path. Jolt does not inspect its schema.

The control/data split is:

```text
control plane: signed identity path or append record -> envelope CID
private plane: encrypted index envelope + recipient wraps
content plane: lazily fetched referenced object CIDs
```

An application can enumerate encrypted index references, fetch envelope bytes
by CID, and ask the daemon to open them with the signed path supplied as
authorization context. Index synchronization MUST NOT imply eager download of
every referenced body.

## 13. Historical Access and Rewrap

When an app asks to open an encrypted object, the daemon exposes one of:

```text
available       a matching local wrap exists and decryption succeeds
needs_rewrap    the object belongs to the local identity but current authorized
                device keys are absent from the historical envelope
not_accessible  no valid local decryption path is known
```

`needs_rewrap` is not plaintext and MUST NOT be treated as proof that rewrap is
possible on this device. Rewrap requires a currently readable historical
object.

To rewrap, an authorized daemon decrypts the historical envelope, computes the
current eligible recipient set, creates a new envelope, and publishes its new
CID at the same application-owned path. The old CID remains immutable.

## 14. Revocation Semantics

After device revocation:

- future private writes MUST exclude the revoked device's keys;
- rewrapped envelopes MUST exclude those keys;
- already published envelopes remain decryptable by any device that retained a
  matching private key;
- already decrypted plaintext cannot be clawed back;
- caches and relays may continue storing old ciphertext.

The honest guarantee is prospective access control, not remote erasure.

## 15. Error Conditions

Implementations SHOULD distinguish unsupported type/version/suite, invalid
author key, author identity mismatch, invalid signature, unsupported recipient
key, missing recipient wrap, invalid nonce, malformed encoding, encryption
failure, and authenticated decryption failure.

Cryptographic failures SHOULD avoid revealing unnecessary oracle detail to an
untrusted app or network caller.

## 16. Compatibility and Versioning

The suite identifier fixes all v1 algorithms. A new algorithm combination
requires a new suite identifier. Changes to canonical field order, domains, or
AAD require a new record version or suite.

Rewrap is migration by republication: the old object remains valid at its CID,
and the signed path moves to a new envelope CID according to JOLT-RFC-0001 or
JOLT-RFC-0003.

## 17. Security Considerations

Implementations MUST use cryptographically secure randomness for CEKs, nonces,
and HPKE encapsulation. Private keys remain daemon-owned and MUST NOT be
returned to apps.

Envelope signatures authenticate the author and envelope metadata; they do not
prove the author was entitled to read an external source document. CIDs protect
serialized bytes from substitution but do not hide equality.

Recipient compromise exposes every envelope wrapped to that key. Revocation
limits future wrapping but cannot cure prior key compromise. Long-lived device
encryption keys SHOULD support rotation in a future revision.

## 18. Privacy Considerations

The envelope exposes author identity, plaintext media type and schema,
declared size, ciphertext size, creation time, recipient identities, recipient
key IDs, and recipient count. Signed paths expose index locations and CIDs.

Applications SHOULD avoid sensitive schema names or path names when those
metadata leaks are unacceptable. This protocol does not provide traffic-flow
confidentiality or recipient anonymity.

## 19. IANA Considerations

This document requests no IANA actions. Suite, record, and domain identifiers
are project-local experiments.

## 20. Implementation Status

Identity encryption-key records, the encrypted envelope, canonical signatures,
HPKE wraps, authenticated encryption, capability-checked publish/open/decrypt/
append/rewrap operations, active-device recipient selection, revoked-device
exclusion, and access-status reporting are implemented.

Generated device private-key custody is still daemon-internal and the current
integration proof focuses on wrap selection. Generated device private keys are
kept only in the in-memory authority store, are not persisted across daemon
restarts, and are not used by the current decrypt path. Authority records are
also held only in memory; after a daemon restart the server bootstraps and
publishes a new legacy-root sequence-zero chain rather than restoring the prior
chain. The decrypt operation also does not enforce `declared_size` against the
returned plaintext length, despite the requirement in Section 10. Group
optimization, key rotation, and production recovery are unresolved. Work was
tracked by cards 052, 096, and 097 and architecture documents 08 and 16.

## 21. References

### 21.1 Normative References

- JOLT-RFC-0001, “Signed Path Records and Resolution.”
- JOLT-RFC-0002, “Device Authorization and Revocation.”
- JOLT-RFC-0003, “Per-Device Writer Logs and Deterministic Merge.”
- [RFC9180] Barnes, R., et al., “Hybrid Public Key Encryption.”
- [RFC8439] Nir, Y. and A. Langley, “ChaCha20 and Poly1305.”
- [RFC8032] Josefsson, S. and I. Liusvaara, “EdDSA.”

### 21.2 Informative References

- Jolt document 16, “Encrypted Object Envelope.”
- Jolt card 097, “Private Content Device Access v0.”

## Appendix A. Access-State Examples

Alice authorizes laptop L and phone P. A private index written afterward has
wraps for L and P and is `available` on both. Alice later authorizes tablet T.
The old index reports `needs_rewrap` on T; new indexes include T automatically.
After L rewraps the old index, the new CID is available on T.

If P is then revoked, later objects and rewraps omit P. The original envelopes
remain decryptable on P if it retained its private key. No conforming interface
may claim otherwise.
