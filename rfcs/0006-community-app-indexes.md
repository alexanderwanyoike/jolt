# Jolt Request for Comments 0006

## Community-Scoped App Indexes and Local Discovery

```text
Jolt Project                                                JOLT-RFC-0006
Request for Comments: 0006                                  August 2026
Category: Experimental
Status: Internet-Draft
Updates: none
Obsoletes: none
```

### Status of This Memo

This document proposes an experimental Jolt community-index protocol. It is not
an IETF publication. Distribution of this memo is unlimited.

The protocol in this memo is not implemented. It records the agreed direction
that discovery comes from signed community state searched by clients, not from
relay-owned global search.

### Abstract

This document defines application-owned indexes published under community
identities. A member signs a generic submission that references application
content by CID. A community curates verified submissions into a signed public
or member-only index without becoming the author of the member payload.

Clients fetch community indexes, verify community curation and original member
signatures, interpret app schemas locally, search locally, and fetch selected
content by CID. Relays may serve or pin the bytes but do not rank results or
control inclusion authority.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Paths and Ownership
6. Member Submission
7. Community App Index
8. Canonical Submission Signature
9. Submission Processing
10. Index Publication and Verification
11. Public and Member-Only Indexes
12. Local Search
13. Removal and Refresh
14. Error Conditions
15. Compatibility and Versioning
16. Security Considerations
17. Privacy Considerations
18. IANA Considerations
19. Implementation Status
20. References
Appendix A. End-to-End Discovery Flow

## 1. Introduction

A decentralized discovery system still needs places where related material can
be found. Making every relay answer global semantic queries would turn relay
operators into index owners and would require the protocol to understand every
application schema.

Jolt communities instead publish signed app-index objects. The community signs
curation: “these member-signed submissions belong in this index.” Members retain
authorship of their individual submissions. The app decides how to interpret,
search, filter, and render the indexed payloads.

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as BCP 14 [RFC2119] [RFC8174]. Primitive binary
encodings use JOLT-RFC-0002 Section 5.

## 3. Scope

This memo defines:

- community-owned app-index paths;
- generic member submission records;
- community index snapshots and original-member proof retention;
- public and member-only index verification;
- local search and lazy content fetch;
- curation, removal, and refresh semantics.

This memo does not define:

- global search, query federation, or relay ranking;
- application payload schemas or ranking functions;
- a protocol-level feed, post, file, game, or gallery;
- automatic moderation;
- storage billing or index-size policy.

## 4. Terminology

**Submission**
: A member-signed generic record referencing an application payload CID.

**Curation**
: A community-authorized decision to include or remove a verified submission.

**Index snapshot**
: The current community-signed collection of included submission proofs for one
  application and visibility class.

**Local discovery**
: Fetching signed indexes and running app-specific search/ranking on the client.

## 5. Paths and Ownership

For an `app_id`, a community uses:

```text
/apps/{app_id}/public-index
/apps/{app_id}/member-index
/apps/{app_id}/submissions
```

`app_id` is an application namespace label and MUST be a non-empty path segment
without `/`, `.` or `..` traversal semantics.

The public and member index paths are community-owned singleton paths. A new
snapshot replaces the previous current CID while history remains available
through signed writer logs. Submission transport MAY use recipient ingress or
an append path; neither creates curation authority.

## 6. Member Submission

The proposed v1 submission is:

```text
CommunityAppSubmission {
  record_type: "jolt.community_app_submission",
  version: 1,
  community_identity: IdentityId,
  app_id: string,
  submission_id: string,
  member_identity: IdentityId,
  member_public_key: bytes[32],
  membership_grant_id: string,
  payload_cid: ContentId,
  payload_media_type: string,
  payload_schema: optional string,
  visibility: public | members,
  created_at: uint64,
  signature: bytes[64]
}
```

The submission says only that one member proposes one CID for one community app
index. It does not make the payload trustworthy, legal, safe, or semantically
valid. Those decisions belong to the community application and curation policy.

## 7. Community App Index

The proposed snapshot is:

```text
CommunityAppIndex {
  record_type: "jolt.community_app_index",
  version: 1,
  community_identity: IdentityId,
  app_id: string,
  visibility: public | members,
  schema: optional string,
  previous_index_cid: optional ContentId,
  entries: [CommunityAppIndexEntry],
  removed_submission_ids: [string],
  generated_at: uint64
}

CommunityAppIndexEntry {
  submission_id: string,
  submission_cid: ContentId,
  member_identity: IdentityId,
  payload_cid: ContentId,
  accepted_at: uint64
}
```

The index object is authorized by the community-signed path binding that names
its CID. The entry repeats selected submission fields for efficient routing,
but the client MUST fetch and verify `submission_cid` before treating the entry
as member-authored.

Entries MUST be sorted by `(submission_id, submission_cid)` byte order and
submission IDs MUST be unique within one snapshot. Removed IDs MUST be sorted
and MUST NOT also appear as active entries.

## 8. Canonical Submission Signature

The member signature payload begins with:

```text
"jolt:community-app-submission:v1" || 0x00
```

It then encodes every submission field except `signature`, in the order shown
in Section 6, using JOLT-RFC-0002 primitives. `visibility` is `0x00` for public
and `0x01` for members.

The receiver MUST derive `member_identity` from `member_public_key` and verify
the Ed25519 signature. The signed submission bytes are serialized and named by
`submission_cid` using JOLT-RFC-0001.

The index snapshot does not replace this signature. The community path proof
attests curation; the submission signature attests member authorship.

## 9. Submission Processing

Before including a submission, a community processor MUST:

1. validate type, version, path-safe app ID, content IDs, and field bounds;
2. verify the member signature and identity binding;
3. resolve JOLT-RFC-0005 membership at the submission's relevant state;
4. require an active grant whose ID equals `membership_grant_id`;
5. require the grant to permit the generic submission action;
6. require submission app ID and visibility to match the target index;
7. apply application-owned schema and moderation policy outside protocol code;
8. publish a new community-authorized snapshot if accepted.

A valid member signature does not require acceptance. Rejection MAY remain a
local curation result; no public rejection record is required by v1.

## 10. Index Publication and Verification

To publish an index, an authorized community device serializes the snapshot,
computes its CID, and sets the corresponding singleton path through
JOLT-RFC-0003. The device writer signature is the community curation proof.

To verify an index, a client MUST:

1. resolve and verify the community identity and writer authority;
2. resolve the expected app-index path;
3. fetch bytes whose hash matches the resolved CID;
4. parse and validate the snapshot identities, app ID, visibility, ordering,
   uniqueness, and size limits;
5. for each used entry, fetch the submission CID and verify Section 8;
6. verify the referenced membership grant and payload CID as needed;
7. treat invalid entries as rejected diagnostics, not silently trusted data.

The snapshot MAY be processed incrementally, but partial processing MUST NOT be
reported as a completely verified index.

## 11. Public and Member-Only Indexes

A public index is plaintext content. Anyone may fetch, cache, pin, verify, and
search it. Public visibility does not imply unrestricted submission.

A member index MUST be a JOLT-RFC-0004 encrypted object whose plaintext is the
snapshot in Section 7. Its recipient wraps target currently active community
members' authorized device encryption keys.

The outer signed path and envelope metadata may reveal that a member index
exists. Membership revocation excludes a device from future snapshots but
cannot retract snapshots or plaintext already obtained.

## 12. Local Search

The protocol search algorithm is intentionally small:

1. discover a community identity from user choice or a replaceable local hint;
2. watch or join according to JOLT-RFC-0005;
3. resolve and verify one or more app-index snapshots;
4. fetch member submissions and any app-owned metadata needed for search;
5. run application-specific query, ranking, and filtering locally;
6. fetch selected payload CIDs lazily.

Relays MAY host community bytes and provider records. They MUST NOT be treated
as the authority for inclusion, membership, ranking, or freshness.

## 13. Removal and Refresh

Removal publishes a new snapshot that omits the active entry and records its
submission ID in `removed_submission_ids`. A client following the latest valid
singleton path MUST stop presenting the removed entry as currently curated.

Historical snapshots and payload CIDs remain immutable and may remain cached.
Removal is a curation change, not network erasure.

Clients SHOULD refresh indexes on explicit user action, app policy, or signed
head change. They SHOULD retain the last verified snapshot when a refresh is
temporarily unavailable and clearly identify it as stale.

## 14. Error Conditions

Implementations SHOULD distinguish malformed submission/index, unsupported
version, invalid member signature, inactive membership, grant mismatch,
community/app/visibility mismatch, duplicate entry, invalid ordering, removed-
and-active conflict, invalid community path proof, encrypted-index access
failure, and payload unavailable.

## 15. Compatibility and Versioning

Application payload schemas evolve independently through `payload_schema`.
Changing generic submission fields, signature encoding, snapshot ordering, or
curation proof requires a new record version.

Clients MAY support several app schemas while sharing the same generic Jolt
submission and index verification layer.

## 16. Security Considerations

Index verification MUST retain the distinction between member authorship and
community curation. A malicious community can curate misleading material but
cannot forge a member signature. A malicious member can submit harmful bytes;
signature validity is not content safety.

Processors require size, count, fetch, and recursion limits. Submission IDs
must be deduplicated to resist retry amplification. Member-only submission
endpoints require verified membership and abuse controls.

## 17. Privacy Considerations

Public indexes reveal community interests, member identities, payload CIDs,
schemas, timing, and removals. Member-only encryption hides snapshot plaintext
from non-recipients but still leaks outer CID, size, timing, and recipient
metadata through the envelope.

Local search avoids sending semantic queries to a relay, but providers can
still observe content fetch patterns.

## 18. IANA Considerations

This document requests no IANA actions. Record names and paths are project-local
experimental identifiers.

## 19. Implementation Status

No community app submission, community index snapshot, curation processor, or
local-index discovery API is implemented. Existing signed paths, membership
design, encrypted envelopes, CIDs, and local application code provide the
foundation.

The exact snapshot representation and rejection reporting remain open to
review. Implementation is tracked by card 100; community membership card 099 is
a prerequisite.

## 20. References

### 20.1 Normative References

- JOLT-RFC-0001, “Signed Path Records and Resolution.”
- JOLT-RFC-0002, “Device Authorization and Revocation.”
- JOLT-RFC-0003, “Per-Device Writer Logs and Deterministic Merge.”
- JOLT-RFC-0004, “Encrypted Objects and Private Device Access.”
- JOLT-RFC-0005, “Community Identities and Membership.”

### 20.2 Informative References

- Jolt card 100, “Community-Scoped App Indexes v0.”
- Jolt document 21, “Community Identity and Membership Model.”

## Appendix A. End-to-End Discovery Flow

Bob signs an app submission referencing payload CID `P` and his active grant.
The community verifies Bob and curates the submission into snapshot CID `I`.
Its authorized device binds `/apps/example/public-index` to `I`. Alice watches
the community, resolves that path, verifies the community writer and Bob's
submission, searches the app metadata locally, and fetches `P` only when she
opens the result. A relay may serve `I` and `P`; it decides none of the trust or
ranking outcomes.

