# Jolt Request for Comments 0003

## Per-Device Writer Logs and Deterministic Merge

```text
Jolt Project                                                JOLT-RFC-0003
Request for Comments: 0003                                  August 2026
Category: Experimental
Status: Internet-Draft
Updates: JOLT-RFC-0001
Obsoletes: none
```

### Status of This Memo

This document specifies an experimental Jolt multi-writer protocol. It is not
an IETF publication. Distribution of this memo is unlimited.

The v1 structures and merge described here are implemented, but the memo
remains a draft because singleton ordering uses wall-clock time and the long-
term logical-clock rule is unresolved.

### Abstract

This document defines per-device append-only writer logs for a Jolt identity.
Each device authorized by JOLT-RFC-0002 writes an independent hash chain. A
resolver verifies those logs against current device authority and materializes
one deterministic identity state independent of provider or discovery order.

The protocol distinguishes singleton path bindings, for which one deterministic
winner is selected, from append path records, which coexist. Jolt merges only
generic signed path operations; applications remain responsible for interpreting
the referenced content.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Device Writer Entry
6. Canonical Signature Encoding
7. Per-Device Log Validation
8. Authority Filtering
9. Deterministic Merge
10. Singleton Resolution
11. Append Enumeration
12. Network Synchronization
13. Error Conditions
14. Compatibility and Versioning
15. Security Considerations
16. Privacy Considerations
17. IANA Considerations
18. Implementation Status
19. References
Appendix A. Concurrent Publish Example

## 1. Introduction

One global sequence counter cannot safely serve several devices that publish
while disconnected. Requiring every device to coordinate before writing would
restore a single-writer bottleneck. Allowing independent devices to extend the
same chain would create forks whose arrival order could change resolved state.

Jolt instead gives each authorized device one independent writer chain. A
resolver first verifies identity authority, then verifies each discovered
device log and merges the resulting generic operations with a deterministic
total order.

This memo supplements the legacy single-writer record in JOLT-RFC-0001. The
user-facing `.jolt` identity remains unchanged.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as BCP 14 [RFC2119] [RFC8174]. Primitive `string`,
`bytes`, `uint16be`, `uint64be`, lists, and optionals use the encoding defined
by JOLT-RFC-0002 Section 5.

## 3. Scope

This memo defines:

- signed per-device writer entries and hash chains;
- singleton and append path modes;
- verification against device authority;
- revoked and unknown device handling;
- deterministic singleton selection and append ordering;
- network exchange of authority records and device logs;
- interaction with legacy signed-path resolution.

This memo does not define:

- application-specific document merging or CRDT semantics;
- tombstones or removal operations;
- a global total order across identities;
- the content schemas referenced by path records;
- durable storage policy for remote identity caches.

## 4. Terminology

**Device writer log**
: One append-only chain whose entries all name one identity and one device.

**Singleton path**
: A path for which the merged identity state exposes one current content ID.

**Append path**
: A path whose independently signed records coexist and are enumerated.

**Entry hash**
: BLAKE3-256 over the canonical entry body, excluding the signature.

**Merged identity state**
: The deterministic singleton map, append-record map, conflict history, and
  rejected-entry diagnostics produced from verified authority and device logs.

## 5. Device Writer Entry

The v1 logical structure is:

```text
DeviceWriterLogEntry {
  body: DeviceWriterLogEntryBody,
  signature: bytes[64]
}

DeviceWriterLogEntryBody {
  record_type: "jolt.device_writer_log_entry",
  version: 1,
  identity: IdentityId,
  device_id: string,
  device_sequence: uint64,
  previous_entry_hash: optional bytes[32],
  operation: SetPath,
  created_at: uint64
}

SetPath {
  path: string,
  content_id: ContentId,
  mode: singleton | append
}
```

`path` MUST be a canonical absolute Jolt path beginning with `/`. Address
normalization MUST NOT silently rewrite a signed path. `content_id` uses the
CID rules from JOLT-RFC-0001.

`created_at` is signed metadata and participates in the implemented v1 merge
order. It is not an authority timestamp and receivers MUST NOT use it to bypass
device authorization or hash continuity.

## 6. Canonical Signature Encoding

The signature payload starts with:

```text
"jolt:device-writer-log-entry:v1" || 0x00
```

The remaining fields are encoded exactly as:

```text
string(record_type)
uint16be(version)
string(identity)
string(device_id)
uint64be(device_sequence)
optional(previous_entry_hash)       ; present value is raw 32 bytes
operation
uint64be(created_at)
```

`SetPath` has discriminator `0x00` followed by:

```text
string(path)
string(content_id_text)
mode                                  ; 0x00 singleton, 0x01 append
```

The entry signature is Ed25519 over the canonical body bytes using the device
signing key from verified authority state. The entry hash is BLAKE3-256 over
those same body bytes and excludes the signature.

## 7. Per-Device Log Validation

For each non-empty candidate device log, a receiver MUST:

1. Validate every entry type, version, device identifier, path, content ID,
   and signature length.
2. Require every entry to name the requested identity.
3. Require every entry to carry the same device identifier as the first entry.
4. Require genesis sequence zero and no previous hash.
5. For each later entry, require its sequence to equal the previous sequence
   plus one.
6. Require its previous hash to equal the preceding entry hash.
7. Find the device in a verified JOLT-RFC-0002 authority state.
8. Verify every signature using that device's authorized signing public key.

A broken chain MUST NOT be truncated and treated as valid. A receiver MAY keep
the bytes for diagnostics, but MUST NOT apply operations from a structurally or
cryptographically invalid log.

## 8. Authority Filtering

An otherwise valid entry from an unknown device is excluded from merged path
state and recorded as rejected with reason `unknown_device`.

For a revoked device, the resolver calls the authority cutoff rule:

```text
active device  -> all valid sequences eligible
revoked device with cutoff N -> sequences <= N eligible
revoked device without cutoff -> no sequence eligible
```

Entries beyond the cutoff are excluded and recorded as rejected with reason
`revoked_device`. Historical entries within the cutoff continue to verify with
the retained device signing key.

## 9. Deterministic Merge

After verification and authority filtering, a receiver partitions `SetPath`
operations by path and mode.

The implemented v1 total ordering key is ascending:

```text
(created_at, device_sequence, device_id, entry_hash)
```

`created_at` and `device_sequence` are unsigned integers, `device_id` is
compared by UTF-8 byte order, and `entry_hash` is compared as 32 raw bytes.

For each singleton path, the greatest tuple is the current winner. Every other
valid candidate is retained as conflict history in ascending tuple order.

For each append path, every valid entry is retained in ascending tuple order.
Discovery order, provider order, map iteration order, and response arrival order
MUST NOT affect the result.

The use of wall-clock `created_at` is an implemented compatibility fact, not a
recommended final design. A future revision SHOULD replace it with a logical
clock while defining an explicit migration and cross-version comparison rule.

## 10. Singleton Resolution

To resolve a `.jolt` address from merged device state, a resolver MUST:

1. require the address identity to equal the merged state identity;
2. normalize the requested address according to JOLT-RFC-0001;
3. look up the normalized path in the singleton winner map;
4. return the winner's content ID and identity/path context;
5. report `path_not_found` if no singleton winner exists.

Append records do not become singleton resolution results merely because their
paths match.

## 11. Append Enumeration

Enumeration accepts an identity and canonical path prefix. A receiver returns
all append records whose signed path begins with that prefix, ordered first by
path byte order and then by the Section 9 entry order.

Enumeration exposes references, not application objects. Each result includes
at least path, content ID, device identifier, device sequence, creation time,
and entry hash. The application fetches and interprets the CID-addressed bytes.

Protocol code MUST NOT infer that append records are posts, replies, messages,
files, or any other app concept.

## 12. Network Synchronization

Device writer state is exchanged over the project-local request/response
protocol `/jolt/device-writer/1.0.0`:

```text
DeviceWriterSyncRequest {
  identity: IdentityId
}

DeviceWriterSyncResponse {
  authority_records: [DeviceAuthorityRecord],
  device_logs: [[DeviceWriterLogEntry]]
}
```

The response is an untrusted candidate. The requester MUST re-verify the
authority chain, every log, and the deterministic merge locally.

Provider discovery reuses the identity update-log provider key. A responder
MAY return partial knowledge. Requesters MAY accumulate different device logs
from several providers, but MUST choose same-device forks deterministically.
The implemented cache chooses equal-length forks by the greatest final entry
order tuple before merging.

## 13. Error Conditions

Implementations SHOULD distinguish:

- unsupported entry type or version;
- identity or device change within a log;
- invalid genesis, sequence, or previous hash;
- empty device identifier or invalid path;
- invalid signature or signature length;
- requested/merged identity mismatch;
- singleton path not found.

Unknown and revoked devices are valid diagnostic rejection outcomes, not a
reason to accept their operations.

## 14. Compatibility and Versioning

Legacy JOLT-RFC-0001 update logs remain readable during migration. A local
legacy identity is represented by the authority device `dev_legacy_root`.
New writes SHOULD populate device-writer state. Resolvers MAY answer from legacy
state while asynchronously warming device-writer state, but MUST identify the
source when diagnostics expose it.

Adding tombstones, new operation classes, or a logical-clock merge key changes
compatibility semantics and requires a versioned update.

## 15. Security Considerations

Resolvers MUST verify authority before writer signatures. A valid Ed25519
signature from an unauthorized device has no identity authority.

Deterministic merge prevents a provider from changing the winner merely by
reordering valid entries. It does not stop withholding attacks: a provider can
omit a newer entry or an entire device log. Implementations SHOULD query more
than one candidate source where freshness matters.

Wall-clock manipulation can influence the v1 singleton winner because
`created_at` is the first ordering element. This is a known protocol weakness
and a primary reason this memo remains a draft.

## 16. Privacy Considerations

Writer logs expose device identifiers, path names, content identifiers,
publication timing, and activity volume even when referenced objects are
encrypted. Applications requiring private indexes SHOULD bind paths to
encrypted objects as described in JOLT-RFC-0004.

## 17. IANA Considerations

This document requests no IANA actions. The protocol name and record strings
are project-local experimental identifiers.

## 18. Implementation Status

The record, canonical encoding, per-device verification, revocation filtering,
singleton conflicts, append coexistence, deterministic merge, local
persistence, remote synchronization, enumeration, and merged-state resolution
are implemented across `jolt-core`, `jolt-network`, `jolt-store`, and
`jolt-server`.

Known gaps are the wall-clock merge key, no tombstone operation, inline rather
than CID-pinned remote log snapshots, and no periodic background synchronization
independent of resolve/enumerate calls. The current importer rejects the whole
imported batch when any supplied writer log is invalid, which is stricter than
the per-log rejection described by this memo. Delivery and verification are
recorded in cards 091 and 094.

## 19. References

### 19.1 Normative References

- JOLT-RFC-0001, “Signed Path Records and Resolution.”
- JOLT-RFC-0002, “Device Authorization and Revocation.”
- [RFC2119] Bradner, S., RFC 2119.
- [RFC8174] Leiba, B., RFC 8174.
- [Ed25519] RFC 8032.

### 19.2 Informative References

- Jolt card 094, “Per-Device Writer Logs and Deterministic Merge v0.”
- Jolt architecture document 20, “True Multi-Writer Identity and Devices.”

## Appendix A. Concurrent Publish Example

Laptop `dev_a` and phone `dev_b` are both authorized. Offline, each appends an
entry under `/spoke/posts/` and sets singleton `/profile`.

After synchronization, both append records remain. For `/profile`, every
conforming resolver compares the same four-part tuple and returns the same
winner. The losing profile binding remains available as conflict diagnostics;
Jolt does not attempt to merge the profile payloads.
