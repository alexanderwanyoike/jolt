# Encrypted Object Envelope

## Status

Design proposal for card `049`.

This document defines the encrypted object envelope that later cards will
implement for private Pastey and future private Jolt apps. It is deliberately
protocol-level and app-agnostic: it talks about encrypted objects, identities,
recipients, keys, and signed paths, not pastes, posts, feeds, or app-specific
schemas.

## Crypto Refresher

Jolt uses three different key roles:

- Identity signing key: proves who authored or authorized something. Today this
  is Ed25519.
- Identity encryption key: lets other people encrypt private objects for this
  identity. v0 uses a separate X25519 encryption key record signed by the
  identity.
- Content encryption key: a random one-time symmetric key generated for one
  encrypted object.

The pattern is:

```text
plaintext object
  -> encrypted once with random content key
  -> content key wrapped once per recipient
  -> envelope signed by author identity key
  -> ciphertext bytes stored by relays/caches
```

This avoids encrypting the same object separately for every recipient. Adding a
recipient means adding a new wrap for the same content key. Revoking someone
from future content means using a new content key or group key for new objects;
it cannot make already-downloaded plaintext disappear from that person's device.

## Design Goals

- Relays and caches store ciphertext and never participate in decryption.
- The envelope is self-describing enough to reject unsupported crypto suites.
- Content encryption, recipient key wrapping, signatures, and identity key
  records stay separate.
- Apps ask the daemon to encrypt and decrypt. Apps do not choose algorithms and
  do not receive long-term private keys.
- Suite identifiers leave room for a post-quantum-hybrid migration without
  changing signed path publishing.
- The protocol layer stays app-agnostic.

## Non-Goals

- Implementing encryption in this card.
- Designing private groups fully.
- Device-key delegation.
- Remote admin authorization.
- Payment, entitlements, or DRM.
- Hiding metadata such as object size, author identity, or recipient count.

## Standards Basis

The v0 suite should use standard building blocks:

- HPKE, RFC 9180, for recipient key wrapping.
- X25519, RFC 7748, as the v0 public-key encryption primitive through HPKE.
- HKDF-SHA256, RFC 5869, through the HPKE suite.
- ChaCha20-Poly1305, RFC 8439, as the v0 AEAD.
- ML-KEM, NIST FIPS 203, as the future post-quantum KEM direction.

Jolt should not invent a new key exchange construction. If implementation
constraints make HPKE unavailable in Rust, implementation cards should either
pick a maintained HPKE crate or explicitly document the temporary construction
as HPKE-compatible in shape: KEM output plus KDF plus AEAD, with the same suite
separation.

## Suite IDs

Suite IDs are stable strings. They are not UI labels.

```text
jolt.enc.v1.x25519-hkdf-sha256-chacha20poly1305.ed25519
jolt.enc.future.hybrid-x25519-ml-kem768-hkdf-sha256-chacha20poly1305.ed25519
```

The v0 suite means:

- Envelope version: `1`
- Content AEAD: ChaCha20-Poly1305
- Content key: random 32-byte key per encrypted object
- Recipient wrap: HPKE Base mode with X25519, HKDF-SHA256, ChaCha20-Poly1305
- Author signature: Ed25519 over canonical envelope bytes
- Recipient encryption key record: signed X25519 public key record

The future hybrid suite direction means:

- Keep the same envelope shape.
- Add recipient key records that contain both X25519 and ML-KEM public keys.
- Wrap the same content key using a hybrid KEM construction that combines the
  classical X25519 shared secret with the ML-KEM shared secret.
- Treat the hybrid suite as a new suite ID. Do not silently reinterpret v0
  envelopes.

## Identity Encryption Key Records

An identity's signing key is the root of authority. It should not also be the
encryption key.

Each identity publishes signed encryption key records under the normal signed
Jolt path `/.well-known/jolt/encryption-keys`. The record has this logical
shape:

```json
{
  "type": "jolt.identity_encryption_keys",
  "version": 1,
  "identity": "alice.jolt",
  "keys": [
    {
      "key_id": "enc_x25519_2026_06",
      "suite_family": "x25519-hkdf-sha256",
      "public_key": {
        "kty": "OKP",
        "crv": "X25519",
        "bytes_b64u": "..."
      },
      "created_at": 1780579200,
      "not_before": 1780579200,
      "expires_at": null,
      "status": "active"
    }
  ],
  "sequence": 7,
  "signature": "ed25519 signature over canonical record body"
}
```

Rules:

- The record is signed by the identity signing key.
- Resolvers verify the record against the requested identity before returning
  any public encryption keys.
- Recipients are addressed by identity plus encryption `key_id`.
- Decrypting nodes only use local private keys that match a recipient wrap's
  `key_id`.
- Rotation publishes a new active key record. Old private keys may be retained
  locally to read older content.
- A compromised encryption key does not by itself let an attacker sign identity
  updates.

This intentionally replaces older docs that suggested deriving X25519 keys from
Ed25519 identity keys.

## Encrypted Object

The encrypted object is the content stored and addressed by CID. The CID is over
the serialized encrypted envelope, not the plaintext.

Logical shape:

```json
{
  "type": "jolt.encrypted_object",
  "version": 1,
  "suite_id": "jolt.enc.v1.x25519-hkdf-sha256-chacha20poly1305.ed25519",
  "author": {
    "identity": "alice.jolt",
    "public_key": "author Ed25519 public key bytes"
  },
  "plaintext": {
    "media_type": "application/octet-stream",
    "schema": null,
    "declared_size": 1234
  },
  "content_encryption": {
    "alg": "CHACHA20-POLY1305",
    "nonce_b64u": "...",
    "aad": "canonical envelope context excluding ciphertext and signature"
  },
  "ciphertext_b64u": "...",
  "recipients": [
    {
      "recipient_identity": "bob.jolt",
      "recipient_key_id": "enc_x25519_2026_06",
      "wrap_alg": "HPKE-BASE-X25519-HKDF-SHA256-CHACHA20POLY1305",
      "enc_b64u": "HPKE encapsulated key",
      "wrapped_key_b64u": "encrypted 32-byte content key",
      "aad": "canonical wrap context"
    }
  ],
  "created_at": 1780579200,
  "signature": {
    "alg": "Ed25519",
    "sig_b64u": "signature over canonical envelope body"
  }
}
```

Implementation may use CBOR rather than JSON for canonical bytes. The JSON above
is explanatory.

## Canonical Signing and AAD

The author signature covers the complete envelope body except the signature
field itself:

```text
signing_payload = canonical_bytes(envelope without signature)
signature = Ed25519.sign(author_private_key, signing_payload)
```

AEAD additional authenticated data should bind encryption to stable object
context. At minimum, content encryption AAD includes:

- `type`
- `version`
- `suite_id`
- author identity
- plaintext metadata
- created time

Content encryption AAD should not include the recipient list. That allows a
future share operation to create a new signed envelope with the same ciphertext
and content key plus an additional recipient wrap, without re-encrypting the
plaintext. The new envelope still gets a new CID because the signed envelope
bytes changed.

Recipient wrap AAD should include:

- object suite ID
- author identity
- recipient identity
- recipient key ID
- wrap algorithm
- a domain string such as `jolt recipient content key wrap v1`

This means an attacker cannot move ciphertext or wrapped keys into a different
envelope without decryption or signature verification failing.

## Encryption Flow

When an app asks to encrypt private content:

```text
App -> daemon:
  plaintext bytes, intended path, recipient identities, optional schema/media type

Daemon:
  1. Check app session has encrypt:<path> capability.
  2. Resolve and verify each recipient's identity encryption key record.
  3. Generate random 32-byte content key.
  4. Generate random AEAD nonce.
  5. Encrypt plaintext once with content key.
  6. HPKE-wrap the content key for each recipient key.
  7. Include an author/self wrap so the author can read their own object.
  8. Sign the envelope with the author's identity signing key.
  9. Publish encrypted object bytes if the app also has publish:encrypted:<path>.
```

The app never receives the author's private signing key or recipient private
encryption keys. The app may receive the final encrypted object bytes and public
metadata.

## Decryption Flow

When an app asks to decrypt:

```text
App -> daemon:
  encrypted object bytes or fetched content ID, app path/context

Daemon:
  1. Check app session has decrypt:<path> capability.
  2. Parse envelope and reject unsupported version/suite.
  3. Verify author signature over canonical envelope body.
  4. Find a recipient wrap whose recipient identity/key ID matches a local
     private encryption key.
  5. HPKE-open that wrap to recover the content key.
  6. AEAD-open the content ciphertext using envelope AAD.
  7. Return plaintext bytes and verified metadata to the app.
```

If the object is not addressed to a local identity, decryption fails without
revealing which other recipients may be able to decrypt.

## App API Direction

Private operations belong behind capability-checked app APIs, not legacy
trusted `/api/v1/*` endpoints.

Implemented v0 app APIs:

```text
POST /app/v1/encrypted/publish
POST /app/v1/encrypted/append
POST /app/v1/encrypted/decrypt
POST /app/v1/encrypted/open
POST /app/v1/encrypted/rewrap
```

The daemon chooses the suite from local policy and supported recipient keys. Apps
may request a visibility mode such as `public` or `private`, but they do not
select `X25519`, `ML-KEM`, `ChaCha20-Poly1305`, nonce formats, or signature
algorithms.

`/app/v1/encrypted/publish` writes a singleton `.jolt` path. `/app/v1/encrypted/append`
writes an encrypted append record under an app-owned identity path so private
app indexes can be enumerated without Jolt knowing the app schema. Decrypt/open
requests may target either a `.jolt` address or a raw content ID. Raw content ID
targets must include the app path context, and the daemon checks
`decrypt:<path>` before fetching and opening the object.

`/app/v1/encrypted/rewrap` re-encrypts an existing object to its current
recipient set. Recipients are enumerated per identity and include the active
authorized device keys for each recipient identity (doc 20), so a device
added after the original publish can gain access without republishing
plaintext. Decrypt/open responses report an access status; `needs_rewrap`
signals that the local identity is a recipient but the object predates one of
its current device keys, and the app should call rewrap. Rewrap requires
`decrypt:<path>`, `encrypt:<path>`, and `publish:encrypted:<path>` together.

Capabilities:

```text
encrypt:/pastes/*
decrypt:/pastes/*
publish:encrypted:/pastes/*
```

Meaning:

- `encrypt:<path>`: app may ask the daemon to encrypt new objects intended for
  that path scope.
- `decrypt:<path>`: app may ask the daemon to decrypt fetched objects in that
  path scope when the local identity is a recipient.
- `publish:encrypted:<path>`: app may publish encrypted object bytes under that
  path scope.

A `share:<path>` capability (adding or rotating recipient access metadata) is
planned for after v0. It is not implemented and cannot be granted today.

## Relays and Caches

Relays store encrypted object bytes exactly like other content bytes.

They may:

- store ciphertext;
- announce provider records;
- serve ciphertext to anyone who asks by CID;
- pin ciphertext when authorized by owner-signed pin requests.

They must not:

- receive content keys;
- receive recipient private keys;
- decide who may decrypt;
- rewrite recipient lists;
- strip signatures or replace envelopes.

Authorization is cryptographic. A relay can deny storage for local policy
reasons, but it cannot grant access to private content.

## Content Addressing

Encrypted objects are content-addressed by ciphertext envelope bytes:

```text
CID = hash(canonical encrypted envelope bytes)
```

The same plaintext encrypted twice should normally produce different CIDs
because content keys and nonces are random. This is desirable for private
content because deterministic encrypted CIDs would leak equality.

Do not include plaintext hashes in the public envelope by default. A plaintext
hash can help local deduplication but leaks equality information across
recipients and publications.

## Migration and Compatibility

Readers must reject unsupported `version` or `suite_id` values with structured
errors.

Writers should use the daemon's current default suite. The daemon may later
support multiple write suites, but apps should not select them directly.

Future post-quantum migration should happen by:

1. publishing identity encryption key records with hybrid key material;
2. introducing a new hybrid suite ID;
3. producing envelopes whose recipient wraps use the hybrid suite;
4. continuing to read old v0 envelopes while the needed old private keys remain
   available locally.

## Implementation Cards After This

- Card 050: identity encryption key records.
- Card 051: encrypted object implementation v0.
- Card 052: daemon encrypt/decrypt app APIs.
- Card 053: private Pastey v0.

## References

- RFC 9180: Hybrid Public Key Encryption,
  https://www.rfc-editor.org/rfc/rfc9180
- RFC 7748: X25519 and X448,
  https://www.rfc-editor.org/rfc/rfc7748
- RFC 5869: HKDF,
  https://www.rfc-editor.org/rfc/rfc5869
- RFC 8439: ChaCha20 and Poly1305 for IETF Protocols,
  https://www.rfc-editor.org/rfc/rfc8439
- NIST FIPS 203: ML-KEM,
  https://csrc.nist.gov/pubs/fips/203/final
