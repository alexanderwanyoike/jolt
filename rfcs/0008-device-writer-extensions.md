# Jolt Request for Comments 0008

## Device Writer Tombstones, Causal Heads, and Delta Sync

```text
Jolt Project                                                JOLT-RFC-0008
Request for Comments: 0008                                  August 2026
Category: Experimental
Status: Experimental Draft
Updates: JOLT-RFC-0003
Obsoletes: none
```

### Status of This Memo

This document specifies implemented extensions to Jolt's experimental
multi-writer protocol. It is not an IETF publication. Distribution of this
memo is unlimited.

The Tombstone operation, causal-head encoding, operation-level negotiation,
and bounded delta synchronization described here are implemented. The memo
remains a draft with JOLT-RFC-0003 because the wider device-writer protocol is
still experimental.

### Abstract

This document extends JOLT-RFC-0003 with three generic protocol behaviors:
signed logical deletion through Tombstones, causal supersession of observed
singleton heads, and bounded cursor-based synchronization of device-writer
histories.

The extensions are negotiated independently. Operation level 2 introduces
Tombstones, operation level 3 introduces signed observed heads, and sync level
2 introduces delta pages. A provider never removes newer operations to make a
history appear compatible with an older requester.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Compatibility Levels
6. Tombstone Operation
7. Causal Observed Heads
8. Singleton Merge
9. Resolution and Restoration
10. Sync Negotiation
11. Delta Synchronization
12. Error Conditions
13. Compatibility and Versioning
14. Security Considerations
15. Privacy Considerations
16. IANA Considerations
17. Implementation Status
18. References
Appendix A. Delete and Restore Example

## 1. Introduction

JOLT-RFC-0003 defines independent device hash chains containing `SetPath`
operations. That v1 model can publish and deterministically select values, but
it cannot represent deletion, distinguish a causal successor from a concurrent
branch, or refresh a large retained history without repeatedly transferring
the complete logs.

This memo adds those behaviors without teaching the protocol about documents,
posts, feeds, or application schemas. The protocol still handles only signed
identity paths, content identifiers, operation state, and device-log history.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as BCP 14 [RFC2119] [RFC8174]. Primitive encodings,
entry hashes, authority validation, and device-log validation use
JOLT-RFC-0002 and JOLT-RFC-0003.

## 3. Scope

This memo defines:

- a signed `TombstonePath` device-writer operation;
- signed causal references to observed singleton heads;
- merge behavior for superseded and concurrent singleton branches;
- operation-level negotiation and fail-closed history exchange;
- cursor validation, bounded delta pages, and full-history fallback; and
- compatibility with existing v1 entries and requesters.

This memo does not define:

- application deletion fields or application schemas;
- physical erasure of immutable content or remote caches;
- schema-aware field merging or application conflict policies;
- retention and eviction policy for verified remote state; or
- a logical clock shared across identities.

## 4. Terminology

**Operation level**
: The highest device-writer entry behavior a peer can interpret. It is
  independent of the sync level and transport protocol name.

**Sync level**
: The highest device-writer history exchange behavior a peer can interpret.

**Tombstone**
: A signed generic operation making a singleton path's current protocol state
  Deleted without carrying application content.

**Observed head**
: The entry hash of a singleton branch that the writer had incorporated when
  producing a successor.

**Cursor**
: A device identifier, device sequence, and exact entry hash naming a verified
  point in one device log.

## 5. Compatibility Levels

Device-writer operation levels are cumulative:

| Level | Contract |
| --- | --- |
| 1 | JOLT-RFC-0003 `SetPath` entries without observed heads. |
| 2 | Level 1 plus `TombstonePath`. |
| 3 | Level 2 plus non-empty signed `observed_heads`. |

Device-writer sync levels are cumulative:

| Level | Contract |
| --- | --- |
| 1 | Complete authority and device-log response. |
| 2 | Level 1 plus validated cursors, bounded delta pages, heads, and continuation. |

The required operation level of a history is the greatest level required by
any entry in the complete history. Once a history contains a Tombstone or
non-empty observed-head set, later `SetPath` entries do not lower that
requirement.

## 6. Tombstone Operation

`TombstonePath` has this logical shape:

```text
TombstonePath {
  path: string
}
```

Its operation discriminator is `0x01`, followed by the canonical `string(path)`
encoding. The existing `SetPath` discriminator remains `0x00`.

The enclosing entry retains JOLT-RFC-0003's record type, record version 1,
signature domain, device sequence, previous-entry hash, and signature rules.
The canonical path MUST pass the same identity/path validation as `SetPath`.
Changing the path after signing MUST invalidate the signature.

A valid Tombstone participates as a singleton candidate whose state is
Tombstone rather than Present. It contains no content identifier and does not
erase immutable content bytes or append records.

## 7. Causal Observed Heads

The device-writer entry body gains one additive field:

```text
observed_heads: [DeviceWriterLogEntryHash]
```

The list MUST be sorted in ascending raw-byte order and MUST contain no
duplicates. A receiver MUST reject a non-canonical list. Each hash is exactly
32 bytes.

When the list is empty, it is omitted from serialized representations and the
canonical signed bytes remain exactly those of JOLT-RFC-0003 v1. When it is
non-empty, the canonical bytes append the following after `created_at`:

```text
0x00 || "jolt:observed-singleton-heads:v1" || 0x00
uint64be(number_of_heads)
head_0[32] || ... || head_n[32]
```

The complete extension is signature-covered and therefore also contributes to
the entry hash. Changing, reordering, adding, or removing an observed head MUST
invalidate the signature.

Observed heads apply to singleton merge only. Append records continue to
coexist and do not become superseded through this field.

## 8. Singleton Merge

After the authority and per-device checks from JOLT-RFC-0003, a resolver groups
singleton `SetPath` and `TombstonePath` entries by path.

For each path, the resolver marks as superseded:

1. the previous singleton entry for that path in the same verified device log;
   and
2. every entry hash named by a candidate's `observed_heads`.

Candidates not marked as superseded are active heads. More than one active head
means the writes are concurrent. The resolver retains the losing active heads
as conflict history and selects a deterministic current head using the greatest
tuple:

```text
(device_sequence, device_id, entry_hash)
```

`device_id` is compared by UTF-8 byte order and `entry_hash` as 32 raw bytes.
Device wall-clock `created_at` MUST NOT choose a singleton winner. It remains
presentation metadata for append-record ordering.

## 9. Resolution and Restoration

Resolving a path whose current head is Present returns its content identifier.
Resolving a path whose current head is a Tombstone returns the distinct
`path_tombstoned` outcome. A Tombstoned path MUST NOT fall back to a stale
legacy path binding or fetch the formerly referenced content as its current
value.

Restoration is not a special protocol operation. An authorized writer restores
a path by appending a singleton `SetPath` entry that observes the current
Tombstone head. The logical path remains stable and the new immutable content
identifier becomes Present. The Tombstone remains in signed history.

## 10. Sync Negotiation

The device-writer request adds fields that default to the legacy level when
absent:

```text
DeviceWriterSyncRequest {
  identity: IdentityId,
  max_operation_version: uint16 = 1,
  max_sync_version: uint16 = 1,
  cursors: [DeviceWriterCursor] = [],
  authority_records: [DeviceAuthorizationRecord] = [],
  device_logs: [[DeviceWriterLogEntry]] = []
}
```

The response adds:

```text
DeviceWriterSyncResponse {
  required_operation_version: uint16 = 1,
  sync_version: uint16 = 1,
  heads: [DeviceWriterCursor] = [],
  continuation: optional DeviceWriterSyncContinuation,
  authority_records: [DeviceAuthorizationRecord],
  device_logs: [[DeviceWriterLogEntry]]
}
```

If the complete history requires an operation level greater than the
requester's maximum, the provider MUST return the required level and MUST
return no authority records or device logs. It MUST NOT strip Tombstones,
observed heads, or their entries to fabricate a compatible history.

Current transports advertise `/jolt/device-writer/4.0.0`, `3.0.0`, `2.0.0`,
and `1.0.0` in newest-first order. The explicit operation and sync fields are
the semantic negotiation boundary; the older stream names remain available for
peer compatibility.

## 11. Delta Synchronization

A sync-level-2 cursor has this shape:

```text
DeviceWriterCursor {
  device_id: string,
  device_sequence: uint64,
  entry_hash: bytes[32]
}
```

For every supplied cursor, the responder MUST find the named device log and
MUST verify that the entry at that sequence has the exact supplied hash.
Duplicate device cursors, an unknown device, an unavailable sequence, or a hash
mismatch invalidates the delta assumption and causes a complete sync-level-1
response.

For valid cursors, the responder returns entries after each cursor and complete
logs for devices without a cursor. Device logs and cursor sets use stable
device-ID ordering. `heads` names the responder's complete current head for
each non-empty device log, independently of page truncation.

A response page is bounded by both entry count and encoded response bytes. The
implemented default limits are 256 entries and 1 MiB. If more entries remain,
`continuation` contains the last transferred cursor for each progressed device
and preserves the request cursor for the others. The requester submits those
cursors for the next page. If even the first entry cannot fit, the responder
falls back to a complete response so the exchange does not report false
progress.

Every received authority record and device-log entry remains untrusted. The
requester MUST perform the normal authority, chain, signature, operation, and
merge verification before retaining or applying it.

## 12. Error Conditions

Implementations SHOULD distinguish:

- a Tombstoned path from a Missing path;
- unsupported required operation level;
- unsupported sync level;
- non-canonical observed heads;
- an invalid or divergent cursor; and
- ordinary authority, chain, path, or signature failure.

An incompatible operation level is not permission to apply a partial history.
A divergent cursor is a synchronization fallback condition, not proof that the
remote history is invalid.

## 13. Compatibility and Versioning

New readers consume level-1 histories unchanged. Empty `observed_heads` retain
the exact legacy canonical bytes and hashes. Missing request and response
negotiation fields decode to level 1.

Old readers cannot correctly materialize a history after it adopts a higher
operation level. Supporting providers therefore fail closed with an empty
history payload rather than presenting false Missing or resurrecting a deleted
value.

Operation levels describe entry semantics. Sync levels describe transfer
semantics. A future extension MUST increase the relevant level and preserve the
same no-downgrade rule when removing the extension would change materialized
state.

## 14. Security Considerations

Tombstones and observed heads have authority only as signed entries from a
currently eligible device sequence. A provider cannot create, alter, reorder,
or remove their signed fields without detection, although it can still withhold
an entire valid history.

An authorized writer that observes a head is allowed to supersede it. This is
the same identity-write authority that allows it to publish a replacement or
Tombstone; causal metadata does not grant authority over another identity.

Cursor hashes prevent a sequence number alone from accepting a different
same-device fork. Delta responses remain subject to complete local
re-verification and MUST NOT be spliced into trusted state solely because a
cursor matched.

## 15. Privacy Considerations

Tombstones reveal that a path was deleted and retain its timing, writer, and
history metadata. Observed heads reveal which branch hashes a writer had seen.
Delta cursors reveal the device logs and sequences already held by a requester.
This memo does not hide those protocol metadata.

Logical deletion does not claw back content already cached, decrypted, or
copied by another party.

## 16. IANA Considerations

This document requests no IANA actions. Record strings, separators, operation
levels, sync levels, and stream protocol names are project-local experimental
identifiers.

## 17. Implementation Status

Operation levels 2 and 3 and sync level 2 are implemented across `jolt-core`,
`jolt-network`, `jolt-store`, and `jolt-server`. Verification covers signed
Tombstones, tampering, persistence and restart, stale legacy resolution,
restore, canonical observed heads, concurrent branches, deterministic
convergence, operation-level refusal, cursor divergence, bounded pages, and
complete-history fallback.

Application schemas and conflict policies remain above this protocol boundary
in the Data SDK. Delivery and verification are recorded in cards 129, 130,
111, and 112.

## 18. References

### 18.1 Normative References

- JOLT-RFC-0002, “Device Authorization and Revocation.”
- JOLT-RFC-0003, “Per-Device Writer Logs and Deterministic Merge.”
- [RFC2119] Bradner, S., RFC 2119.
- [RFC8174] Leiba, B., RFC 8174.

### 18.2 Informative References

- Jolt card 129, “Protocol Tombstone, Delete, and Restore.”
- Jolt card 130, “Schema-Aware Concurrency Policies.”
- Jolt card 111, “Bounded Remote Identity Sync.”
- Jolt card 112, “Device-Writer Delta Sync.”

## Appendix A. Delete and Restore Example

An authorized device first publishes singleton `SetPath("/notes/1", CID_A)`.
Deleting the logical record appends `TombstonePath("/notes/1")` while observing
the Present head. Resolution now returns `path_tombstoned`; CID_A remains an
immutable historical object.

A later restore publishes `SetPath("/notes/1", CID_B, singleton)` while
observing the Tombstone head. Resolution returns CID_B. A peer must support
operation level 2 to receive the complete history, even though its current head
is again a `SetPath` entry.
