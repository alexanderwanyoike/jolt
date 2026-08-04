# Jolt Request for Comments 0001

## Jolt Signed Path Records and Resolution

```text
Jolt Project                                                JOLT-RFC-0001
Request for Comments: 0001                                  August 2026
Category: Experimental
Status: Internet-Draft
Updates: none
Obsoletes: none
```

### Status of This Memo

This document specifies an experimental protocol for the Jolt project and
requests discussion and implementation feedback. It is not an IETF publication
and has not been reviewed or approved by the IETF. Distribution of this memo is
unlimited.

This document is a draft. Implementations MUST NOT treat the protocol elements
defined here as permanently frozen until this memo reaches Accepted status.

### Abstract

This document defines the Jolt signed-path protocol. A Jolt identity is derived
from an Ed25519 public key. The identity controls a hash-chained sequence of
signed records that maps application-agnostic paths to content identifiers.
Content identifiers use CIDv1 with the raw codec and BLAKE3-256 multihashes.

This document specifies identity and address syntax, path normalization,
content identifier construction, the signed record data model, the canonical
binary encoding covered by signatures, record-chain validation, state replay,
address resolution, selection among untrusted candidate logs, protocol errors,
and security requirements.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Primitive Types and Encoding Rules
6. Identity Identifiers and Jolt Addresses
7. Content Identifiers
8. Signed Path Record Format
9. Canonical Signature Encoding
10. Record Construction
11. Record-Chain Validation
12. State Replay
13. Address Resolution
14. Candidate Selection and Staleness
15. Error Conditions
16. Security Considerations
17. Privacy Considerations
18. IANA Considerations
19. Implementation Status
20. References
Appendix A. End-to-End Protocol Operation
Appendix B. Architectural Boundaries
Appendix C. Implementation Mapping

## 1. Introduction

Hosted applications commonly combine identity, mutable naming, object storage,
distribution, and presentation. Jolt separates the mutable identity-owned name
from the immutable object and from the application that interprets the object.

The protocol statement defined by this memo is:

```text
identity X maps path P to content identifier C at sequence N
```

The statement is authorised by a signature from identity `X`. The bytes named
by `C` are verified by the content identifier. The protocol does not define what
the bytes mean. An application may interpret them as a profile, post, gallery,
release, message envelope, game manifest, or another schema.

The signed-path protocol is designed for operation with untrusted peers,
caches, and relays. A provider can withhold data or return stale data, but it
cannot create a valid newer mapping without an authorised signing key.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **NOT RECOMMENDED**, **MAY**, and
**OPTIONAL** in this document are to be interpreted as described in BCP 14
[RFC2119] [RFC8174] when, and only when, they appear in all capitals.

Unsigned integers are written as `uintN`, where `N` is the width in bits.
`uint64be` is an unsigned 64-bit integer encoded in network byte order.

Hexadecimal octets are written using the notation `0xNN`. Literal byte strings
are enclosed in double quotes. `NUL` means the single octet `0x00`.

## 3. Scope

This memo defines:

- Ed25519-rooted Jolt identity identifiers;
- textual `.jolt` addresses;
- path validation and normalization;
- CIDv1 raw/BLAKE3-256 content identifiers;
- legacy single-writer signed path records;
- the exact byte sequence covered by an Ed25519 signature;
- record hash chaining, validation, replay, and resolution;
- deterministic selection among candidate single-writer logs.

This memo does not define:

- discovery of candidate providers;
- libp2p, iroh, relay, or cache transport messages;
- multi-device authority or per-device writer-log merging;
- encrypted object envelopes or access grants;
- app-session capabilities or daemon HTTP APIs;
- application schemas or media types.

Those surfaces require separate specifications. In particular, the
single-writer log in this memo is the implemented v0 compatibility surface and
is expected to be supplemented or superseded by the multi-device writer
protocol.

## 4. Terminology

**Identity public key**
: A 32-octet Ed25519 public verification key.

**Identity identifier**
: The lowercase, unpadded Base32 encoding of an identity public key.

**Jolt address**
: An identity identifier followed by `.jolt` and an optional normalized path.

**Content identifier** or **CID**
: A CIDv1 value using multicodec `raw` and a BLAKE3-256 multihash.

**Record**
: A `SignedPathRecord` containing a record body and an Ed25519 signature.

**Record body**
: The identity public key, sequence, previous-record hash, and action covered by
  the record signature.

**Genesis record**
: The first record in a log. Its sequence is zero and it has no previous-record
  hash.

**Record hash**
: BLAKE3-256 over the canonical signature encoding of a record body. The
  signature itself is not included.

**Verified log**
: A non-empty ordered record sequence satisfying every requirement in Section
  11.

**Resolved state**
: The path map produced by applying all actions in a verified log in sequence.

**Provider**
: Any peer, cache, or relay that supplies records or content. A provider is not
  an authority merely because it supplies data.

## 5. Primitive Types and Encoding Rules

### 5.1 Octet Strings

The canonical signed encoding uses the following length-prefixed octet-string
form:

```text
octets = length content
length = uint64be
content = length octets
```

The length is the number of content octets and MUST be encoded as an unsigned
64-bit big-endian integer. An implementation MUST reject a length that exceeds
its configured resource limit before allocating storage.

### 5.2 UTF-8 Strings

A string is encoded as its UTF-8 octets using the `octets` form from Section
5.1. Strings MUST contain valid UTF-8. Unless a field definition states
otherwise, strings are compared as exact octet sequences and are not Unicode
normalized.

### 5.3 Optional Values

An optional value is encoded with a one-octet discriminator:

```text
0x00                 ; absent
0x01 value           ; present
```

Any other discriminator MUST be rejected.

### 5.4 Lists

A list is encoded as a `uint64be` item count followed by each item in order.
An implementation MUST apply configured limits to the count before allocating.

## 6. Identity Identifiers and Jolt Addresses

### 6.1 Identity Identifier Construction

Given a 32-octet Ed25519 public key `K`, the identity identifier is:

```text
lowercase(BASE32-NOPAD(K))
```

The Base32 alphabet is the alphabet from [RFC4648], Section 6. Padding MUST NOT
be emitted or accepted. Because the input is exactly 32 octets, the resulting
identifier is exactly 52 ASCII characters.

An identity parser MUST reject a value that:

- is not exactly 52 characters;
- contains uppercase characters;
- contains characters outside the unpadded Base32 alphabet;
- does not decode to exactly 32 octets.

### 6.2 Address Syntax

The syntax is specified using ABNF [RFC5234]:

```abnf
jolt-address  = identity-label ".jolt" [ path ]
identity-label = 52(base32-lower)
base32-lower  = %x61-7A / %x32-37
path          = "/" [ path-segment *( "/" path-segment ) ]
path-segment  = 1*( path-char )
path-char     = %x21-22 / %x24-2E / %x30-3E / %x40-7E / UTF8-non-ascii
```

`UTF8-non-ascii` means a well-formed UTF-8 sequence encoding a Unicode scalar
value above U+007F. The `path-char` production excludes ASCII whitespace, `#`,
`/`, and `?`. Applications SHOULD use portable URI unreserved characters for
newly created paths.

The identity label MUST be the only label preceding `.jolt`. Implementations
MUST reject names such as `sub.<identity>.jolt`.

### 6.3 Path Normalization and Validation

The absent path and `/` both normalize to `/`.

If a programmatic API receives a non-empty path that does not begin with `/`, it
MAY prepend `/`. A textual address parser MUST interpret the first `/` after
`.jolt` as the beginning of the path.

A path MUST NOT contain:

- any Unicode whitespace character;
- a query delimiter `?`;
- a fragment delimiter `#`;
- a segment equal to `.`;
- a segment equal to `..`.

Path comparison is exact and case-sensitive after the normalization above.
Implementations MUST NOT percent-decode, Unicode-normalize, collapse repeated
slashes, or resolve dot segments during comparison.

## 7. Content Identifiers

For content bytes `B`, a conforming writer MUST construct the content
identifier as:

```text
CIDv1(
  codec = raw (0x55),
  multihash = blake3-256(B)
)
```

The text representation MUST use the normal CID string representation. A
reader MUST parse the CID and MUST verify that:

- the CID version is 1;
- the multicodec is `raw` (`0x55`);
- the multihash algorithm is BLAKE3-256;
- hashing the received bytes produces the same multihash digest.

A reader MUST NOT return or cache object bytes as a successful fetch when CID
verification fails.

## 8. Signed Path Record Format

### 8.1 Record

The abstract record format is:

```text
SignedPathRecord {
  body: RecordBody,
  signature: octet[64]
}

RecordBody {
  owner_public_key: octet[32],
  sequence: uint64,
  previous_record_hash: optional octet[32],
  action: Action
}
```

`signature` is an Ed25519 signature over `CanonicalRecordBody(body)` as defined
in Section 9.

### 8.2 Actions

This memo defines the following action tags:

| Tag | Name | Fields | Status |
|---:|---|---|---|
| 0 | `PublishContent` | `content_id` | Legacy; no resolved-state effect |
| 1 | `UpdateRoot` | `content_id` | Legacy root pointer |
| 2 | `UpdateProfile` | profile fields | Deprecated application-specific action |
| 3 | `SetPath` | `path`, `content_id` | Current generic write action |
| 4 | `RemovePath` | `path` | Current generic removal action |
| 5 | `SetReachability` | relay hints | Legacy reachability action |

New application data MUST use `SetPath` and `RemovePath`. New protocol
specifications MUST NOT allocate application-specific actions in this record
type. Tag 2 is retained only to describe and verify existing v0 logs.

### 8.3 SetPath

`SetPath(path, content_id)` sets the current mapping for `path` to
`content_id`. The path MUST satisfy Section 6.3. The content identifier MUST
satisfy Section 7.

### 8.4 RemovePath

`RemovePath(path)` removes the current mapping for `path`. Removing an absent
path is valid and has no effect.

### 8.5 Legacy Actions

Readers of v0 logs MUST be able to validate and skip `PublishContent`, apply
`UpdateRoot` to a separate legacy root field, decode `UpdateProfile` into a
separate legacy profile field, and replace legacy relay hints for
`SetReachability`. Application-agnostic resolvers MUST NOT reinterpret those
legacy fields as path mappings.

## 9. Canonical Signature Encoding

### 9.1 Record Body

`CanonicalRecordBody(body)` is the concatenation below with no alignment or
padding:

```text
"jolt:update-log-entry:v1" NUL
octets(owner_public_key)
uint64be(sequence)
optional_previous_hash
canonical_action
```

`optional_previous_hash` is `0x00` when absent and `0x01` followed by 32 hash
octets when present.

### 9.2 Action Encoding

The canonical action encodings are:

```text
PublishContent = 0x00 string(content_id)
UpdateRoot     = 0x01 string(content_id)
UpdateProfile  = 0x02 optional_string(display_name)
                       optional_string(bio)
                       optional_string(avatar_cid)
SetPath        = 0x03 string(path) string(content_id)
RemovePath     = 0x04 string(path)
SetReachability = 0x05 relay_hint_list
```

An `optional_string` uses Section 5.3 followed by a Section 5.2 string when
present.

`relay_hint_list` is encoded as a Section 5.4 list. Each relay hint is:

```text
string(relay_identity)
string(peer_id)
list(string(address))
list(capability_tag)
optional(uint64be(expires_at))
```

Relay capability tags are `0x00` Discovery, `0x01` Pinning, and `0x02`
Serving. Unknown action or capability tags MUST be rejected by this protocol
version.

### 9.3 Record Hash

The record hash is:

```text
BLAKE3-256(CanonicalRecordBody(body))
```

The 64-octet signature is deliberately excluded. This produces a stable chain
link for a given body independent of signature representation.

### 9.4 Signature

The record signature is:

```text
Ed25519-Sign(owner_private_key, CanonicalRecordBody(body))
```

Readers MUST verify the signature against `owner_public_key`. The public key
MUST be exactly 32 octets and the signature MUST be exactly 64 octets.

## 10. Record Construction

### 10.1 Genesis Record

A writer constructing the first record MUST set:

```text
sequence = 0
previous_record_hash = absent
owner_public_key = identity public key
```

It MUST validate the action and then sign the canonical body.

### 10.2 Appended Record

Given a previous record `R`, an appended record MUST set:

```text
owner_public_key = R.body.owner_public_key
sequence = R.body.sequence + 1
previous_record_hash = RecordHash(R.body)
```

The writer MUST fail rather than wrap if the previous sequence is `2^64 - 1`.

## 11. Record-Chain Validation

Given an ordered sequence `L`, a reader MUST perform all of the following:

1. Reject an empty sequence when resolution is requested.
2. Check that every public key is exactly 32 octets.
3. Check that every signature is exactly 64 octets.
4. Verify every signature over the canonical record body.
5. Require `L[0].sequence == 0`.
6. Require `L[0].previous_record_hash` to be absent.
7. Require every record to carry the same `owner_public_key` as `L[0]`.
8. For each record `L[i]` where `i > 0`, require
   `L[i].sequence == L[i-1].sequence + 1`.
9. For each record `L[i]` where `i > 0`, require
   `L[i].previous_record_hash == RecordHash(L[i-1].body)`.
10. Validate the syntax and values of every action.

Validation MUST fail closed. A reader MUST NOT replay a valid prefix and ignore
an invalid suffix while presenting the result as the supplied log's latest
state. A transport MAY request a known valid prefix separately.

## 12. State Replay

After validation, a reader initializes:

```text
latest_sequence = 0
paths = empty map
legacy_root = absent
legacy_profile = absent
legacy_reachability = empty list
```

It then applies records in ascending sequence order:

- `SetPath(P, C)`: set `paths[P] = C`, replacing any previous value.
- `RemovePath(P)`: remove `P` from `paths` if present.
- `UpdateRoot(C)`: set `legacy_root = C`.
- `UpdateProfile(V)`: set `legacy_profile = V`.
- `SetReachability(R)`: replace `legacy_reachability` with `R`.
- `PublishContent(C)`: no resolved-state effect.

After each record, `latest_sequence` becomes that record's sequence.

## 13. Address Resolution

To resolve address `A` against candidate log `L`, a resolver MUST:

1. Parse and validate `A` under Section 6.
2. Validate `L` under Section 11.
3. Derive the identity identifier from `L[0].owner_public_key`.
4. Require the derived identifier to equal the identity in `A`.
5. Replay `L` under Section 12.
6. Look up the normalized address path in `paths`.
7. Return the content identifier and latest sequence when present.
8. Return `path-not-found` when absent.

Resolution does not fetch the object. A consumer that subsequently fetches the
object MUST perform the CID checks in Section 7 before using or caching it.

The root address `<identity>.jolt` normalizes to path `/`. It resolves only when
the log contains a `SetPath` action for `/`. The legacy `UpdateRoot` field MUST
NOT implicitly satisfy a generic path resolution request.

## 14. Candidate Selection and Staleness

Providers are untrusted and MAY return empty, invalid, truncated, stale, or
conflicting candidate logs.

For each candidate, a resolver MUST validate the full chain and require that
the owner matches the requested identity. Invalid candidates are discarded.

Among remaining single-writer candidates, the resolver MUST select the
candidate with the greatest final sequence number.

If two valid candidates for the same identity have the same final sequence but
different final record hashes, the resolver MUST return a `fork-detected` error
unless local policy has an explicitly specified recovery rule. It MUST NOT pick
one based on arrival order.

A provider can suppress newer valid state. The protocol therefore provides
integrity and monotonic comparison, not a guarantee that a resolver has found
the globally newest state. A resolver SHOULD retain the highest verified
sequence previously observed for each identity and SHOULD warn or reject when a
later query attempts to downgrade it.

## 15. Error Conditions

Implementations SHOULD expose errors at least as specifically as the following
codes:

| Code | Meaning |
|---|---|
| `invalid-identity` | Identity label is not canonical Base32 for 32 octets |
| `invalid-address` | Address suffix, label count, or syntax is invalid |
| `invalid-path` | Path violates Section 6.3 |
| `invalid-content-id` | CID does not satisfy Section 7 |
| `invalid-public-key` | Owner key is not a valid 32-octet Ed25519 key |
| `invalid-signature-length` | Signature is not 64 octets |
| `invalid-signature` | Ed25519 verification failed |
| `invalid-genesis` | Genesis sequence or previous hash is invalid |
| `owner-changed` | A later record changes the owner public key |
| `sequence-gap` | Sequence is not previous sequence plus one |
| `broken-chain` | Previous-record hash does not match |
| `unknown-action` | Action tag is not defined for this version |
| `identity-mismatch` | Address identity differs from log owner |
| `path-not-found` | No current mapping exists for the path |
| `fork-detected` | Equal-sequence valid candidates have different heads |
| `content-mismatch` | Object bytes do not match the requested CID |
| `resource-limit` | Length or count exceeds local safe limits |

Error strings are not protocol elements and MAY be localized.

## 16. Security Considerations

### 16.1 Key Compromise

Possession of the Ed25519 private key permits creation of valid signed records
for the identity. This single-writer protocol has no key-rotation or revocation
mechanism. Implementations MUST protect private keys at rest and in use. The
current Jolt v0 implementation has not completed independent review of its key
storage and MUST NOT be described as production-hardened.

### 16.2 Signature Domain Separation

The `jolt:update-log-entry:v1` NUL-terminated domain separator prevents a record
signature from being interpreted as a signature over another Jolt structure.
Implementations MUST include it exactly.

### 16.3 Replay and Rollback

Valid historical logs can be replayed by a malicious provider. Retaining the
highest previously verified sequence mitigates rollback but does not prove that
no unseen newer state exists.

### 16.4 Equivocation

An identity key can sign two different records at the same sequence with the
same previous hash. This is detectable when both heads are observed but is not
prevented by the single-writer format. Implementations MUST surface equal-height
forks as specified in Section 14.

### 16.5 Hash and Content Verification

Transport security and provider identity do not replace CID verification. A
consumer MUST hash received bytes before use. A cache MUST NOT store bytes under
a CID they do not match.

### 16.6 Resource Exhaustion

All variable-length fields carry 64-bit lengths or counts. Implementations MUST
enforce configured upper bounds before allocation, signature verification, log
replay, or content fetch. Implementations SHOULD cap path length, string length,
records per response, and total candidate bytes.

### 16.7 Path Confusion

Implementations MUST apply the exact path rules in Section 6.3. They MUST NOT
apply filesystem normalization or use an unvalidated Jolt path directly as a
local filesystem path.

### 16.8 Deprecated Application Action

The legacy `UpdateProfile` action violates the current application-agnostic
protocol boundary. It is specified only so existing records can be validated.
Writers MUST NOT create it in new logs.

## 17. Privacy Considerations

Identity identifiers are stable public-key-derived identifiers and are
therefore linkable wherever reused. Public path names, update frequency,
sequence numbers, object sizes, provider announcements, and access timing can
reveal metadata even when referenced content is encrypted.

This memo does not provide confidentiality. Applications requiring private data
MUST use an encrypted-object protocol and SHOULD avoid descriptive public path
names when those names reveal sensitive information.

## 18. IANA Considerations

This document has no IANA actions. The `.jolt` suffix is an application-level
identifier in this experimental protocol and is not requested as a DNS
top-level domain by this memo.

## 19. Implementation Status

The Jolt Rust implementation contains the identity encoding, address parser,
CID construction, canonical record encoding, Ed25519 verification, hash-chain
validation, state replay, and highest-sequence candidate selection described by
this memo.

The following gaps are known at draft time:

- equal-height fork detection is not yet a durable public error contract;
- generic CID parsing and the network fetch/cache path require an audit to ensure
  the codec and digest constraints in Section 7 are enforced before content is
  returned or cached;
- resource limits are implementation policy rather than protocol constants;
- legacy action tags remain present in the code;
- the transition to per-device writer logs is specified separately and may
  supersede parts of Sections 8 through 14;
- independent interoperability test vectors have not yet been published;
- the implementation has not received an independent security audit.

Acceptance of this memo requires published binary test vectors for canonical
encoding, signatures, record hashes, address parsing, replay, and failure cases.

## 20. References

### 20.1 Normative References

[RFC2119] Bradner, S., "Key words for use in RFCs to Indicate Requirement
Levels", BCP 14, RFC 2119, March 1997.

[RFC4648] Josefsson, S., "The Base16, Base32, and Base64 Data Encodings", RFC
4648, October 2006.

[RFC5234] Crocker, D. and P. Overell, "Augmented BNF for Syntax
Specifications: ABNF", STD 68, RFC 5234, January 2008.

[RFC8032] Josefsson, S. and I. Liusvaara, "Edwards-Curve Digital Signature
Algorithm (EdDSA)", RFC 8032, January 2017.

[RFC8174] Leiba, B., "Ambiguity of Uppercase vs Lowercase in RFC 2119 Key
Words", BCP 14, RFC 8174, May 2017.

[CID] Multiformats, "Content Identifiers", current CID specification.

[BLAKE3] O'Connor, J., Aumasson, J., Neves, S., and Z. Wilcox-O'Hearn,
"BLAKE3: one function, fast everywhere", current specification.

### 20.2 Informative References

[JOLT-ARCH] Jolt project, `docs/01-architecture.md`.

[JOLT-IDENTITY] Jolt project, `docs/02-identity-and-crypto.md`.

[JOLT-DATA] Jolt project, `docs/05-data-model.md`.

[JOLT-RESOLUTION] Jolt project, `docs/12-global-jolt-resolution.md`.

## Appendix A. End-to-End Protocol Operation

This appendix is informative. It places the normative signed-path protocol in
the complete publish, resolve, fetch, and availability loop described by the
Jolt architecture and content-distribution documents.

### A.1 Authority and Data Planes

A Jolt node separates three kinds of state:

**Control-plane state**
: Identity keys, signed update records, current path bindings, sequence
  information, and reachability hints. This state is small, mutable, and
  security-sensitive. It determines which immutable objects an identity
  currently names.

**Content-plane state**
: Immutable bytes addressed by CID. Content bytes may be held by the author,
  another peer, a cache, or a relay. Their storage location does not change
  their identity or authorisation.

**Application state**
: Schemas and records whose meaning belongs to an application. A Jolt record
  can prove that an identity mapped “/spoke/posts/abc” to a CID. The meaning of
  “post”, its fields, and its presentation remain Spoke concerns.

The separation is deliberate. A provider can carry control-plane and
content-plane bytes without becoming an identity authority. An application can
interpret and create application state without receiving the identity's
private key.

### A.2 Publication Procedure

An end-to-end publisher performs the following procedure when assigning new
bytes B to path P:

1. Validate and normalize P according to Section 6.3.
2. Store B in the local content store.
3. Construct C = CIDv1(raw, BLAKE3-256(B)) according to Section 7.
4. Read the current valid local head for the publishing identity.
5. Construct SetPath(P, C) as the next record according to Section 10.
6. Sign the canonical record body according to Section 9.4.
7. Verify the newly constructed record using the same validation path used for
   received records. Writers SHOULD NOT maintain a less strict local-only
   validation path.
8. Durably append the record before announcing it to other nodes.
9. Announce or upload the content and signed record through the configured
   provider mechanisms.
10. If an owner-selected relay is configured, request that it pin both the
    referenced content and enough signed log state to answer resolution while
    the publisher is offline.

Steps 8 through 10 are operational requirements rather than signed-record wire
format. A failure after the durable local append does not invalidate the
record. It means the new state is valid but may not yet be discoverable or
available from another node.

Publishing a replacement for P does not mutate the old object. It creates a new
CID and a new signed path record. The old bytes may remain cached or pinned
according to local policy.

### A.3 Candidate Discovery

Discovery answers only:

    which nodes might have records for identity X?

It does not answer:

    which record is valid for identity X?

A resolver can obtain candidate logs from any combination of:

- its local verified cache;
- a record supplied with an invite or link;
- a known home relay;
- peers already associated with the identity;
- a provider-discovery mechanism keyed by the identity; or
- an opportunistic cache.

Candidate sources MAY be malicious. Every candidate therefore enters the same
validation and selection algorithm in Sections 11 and 14. A resolver MUST NOT
skip signature or chain validation because a candidate arrived from a
configured home relay, a transport-authenticated peer, or local cache.

The discovery layer may return no candidates even when valid state exists. This
is an availability failure, not evidence that the identity or path does not
exist.

### A.4 Resolution and Fetch Procedure

For address “identity.jolt/path”, an end-to-end consumer performs:

1. Parse the address and derive the requested identity and normalized path.
2. Load the highest previously verified local candidate, if present.
3. Discover additional candidate providers.
4. Request complete logs or extensions from a known verified head.
5. Validate every candidate as specified in Section 11.
6. Select the winning candidate as specified in Section 14.
7. Replay it and resolve the path as specified in Sections 12 and 13.
8. Check the local content store for the resolved CID.
9. If absent, discover providers for the CID.
10. Request bytes from one or more providers according to local transport
    policy.
11. Compute the required BLAKE3-256 digest over the complete received object.
12. Reject the response unless the computed CID exactly equals the requested
    CID.
13. Only after successful CID verification, return the bytes to the caller and
    optionally enter them into the local cache.

A consumer MUST distinguish at least these failures:

- no state candidates were discovered;
- candidates were discovered but none validated;
- a valid identity state did not contain the requested path;
- the path resolved but no content provider was reachable; and
- bytes were received but failed CID verification.

Collapsing these cases into “not found” obscures security failures and makes
availability diagnosis needlessly difficult.

### A.5 Availability and Relay Pinning

Jolt cannot make bytes available without an online node that possesses and is
willing to serve them. Its availability rule is therefore:

    content is available while at least one willing provider that has it is online

The protocol distinguishes:

**Cache**
: An opportunistic copy. The caching node may evict it under local policy.

**Pin**
: An intentional local or owner-requested promise to retain an object. A relay
  may refuse a request because of quota, policy, signature failure, or
  unsupported features.

**Mirror**
: Owner-authorized relay-to-relay durable replication. This is future work and
  is not defined by this memo.

A home relay is a replaceable availability provider selected by the owner. It
may keep content and signed records online, announce itself as a provider, and
serve requests while the owner's personal device is offline. It does not become
the authority for the identity, path mapping, or content.

Relays MUST NOT rewrite signed records. A consumer MUST NOT accept a relay's
unsigned assertion that one path replaces another. Relay selection can change
without changing the identity identifier, record signatures, or content IDs.

### A.6 Encrypted Objects

This memo identifies bytes, not plaintext. An application may publish a
canonical encrypted-object envelope as the content bytes named by a CID.

In that case:

- the CID verifies the complete encrypted envelope bytes;
- an author signature inside the envelope authenticates its author and
  metadata;
- recipient key wraps determine which identities can recover the content key;
- relays and caches may store and serve the ciphertext without receiving
  content keys; and
- successful CID verification does not imply that the local identity is
  authorized to decrypt the object.

Encryption suites, identity encryption-key records, recipient wrapping, and
envelope canonicalization are outside this memo and require their own
specification. Implementations MUST NOT derive encryption behaviour from path
names or application schema names in the signed-path layer.

## Appendix B. Architectural Boundaries

This appendix is informative but records constraints that future Jolt protocol
specifications are expected to preserve.

### B.1 Local Daemon

The local Jolt daemon is the authority boundary on a user's device. It owns or
mediates:

- identity and device private keys;
- signature and verification operations;
- verified update-log state;
- content storage and CID verification;
- encryption and decryption operations;
- application session approval and capability checks;
- peer connections, provider discovery, and relay configuration.

Applications do not receive ambient access to these resources. They submit
operations to the daemon, and the daemon checks the selected identity,
application identity, granted capability, and path scope.

### B.2 Application Boundary

The protocol layer may know about identities, CIDs, signed logs, generic paths,
content fetch, provider discovery, relays, pinning, encryption envelopes,
access grants, capabilities, and schema references.

It MUST NOT make profiles, posts, feeds, timelines, galleries, games,
communities-as-products, or application runtimes part of signed-path semantics.
Those concepts are represented as content and schemas above the protocol.

This permits multiple applications to interpret the same identity-owned graph
without requiring the protocol to adopt one application's vocabulary.

### B.3 Network Boundary

Network peers, caches, and relays are untrusted data sources. Transport
encryption can authenticate a live connection and protect it from passive
observation, but it does not replace:

- Ed25519 verification of mutable identity records;
- chain and sequence validation;
- identity-to-owner matching;
- rollback and fork handling; or
- CID verification of content bytes.

The source of bytes is provenance metadata, not proof of their validity.

### B.4 Current and Target Identity Models

The record format in this memo specifies the current single-writer
compatibility surface. The target architecture gives each authorized device a
separate writer key and append-only writer log under a root identity authority
chain.

That target model must additionally specify:

- device authorization and revocation records;
- the exact accepted sequence boundary for a revoked writer;
- per-device canonical entry encoding;
- deterministic merge ordering independent of discovery order;
- singleton-path conflict selection;
- append-record and tombstone semantics; and
- migration of existing single-writer logs.

Those requirements are not silently inferred by this memo. Until a
multi-writer RFC is accepted, implementations claiming conformance to this memo
must implement the single-writer rules exactly and expose forks rather than
inventing an undocumented merge.

## Appendix C. Implementation Mapping

This appendix is informative. It helps reviewers compare the specification
with the current Rust implementation; file names are not protocol elements.

| Protocol concern | Current implementation area |
|---|---|
| Identity identifier and address parsing | crates/jolt-core/src/identity_address.rs |
| CID construction and parsing | crates/jolt-core/src/content_id.rs |
| Record body, actions, canonical bytes, signing, and replay | crates/jolt-core/src/update_log.rs |
| Update-log exchange messages | crates/jolt-network/src/protocol.rs |
| Local content persistence and cache behaviour | crates/jolt-store |
| Publish, fetch, and provider orchestration | crates/jolt-content and node services |
| Identity key ownership and signing | crates/jolt-identity |
| Capability-checked app boundary | crates/jolt-server session and app API modules |

Conformance is determined by observable protocol behaviour and published test
vectors, not by using these crates or reproducing their internal structure.

## Author's Address

Jolt contributors  
<https://github.com/alexanderwanyoike/jolt>
