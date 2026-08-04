# Jolt Request for Comments 0005

## Community Identities and Membership

```text
Jolt Project                                                JOLT-RFC-0005
Request for Comments: 0005                                  August 2026
Category: Experimental
Status: Internet-Draft
Updates: none
Obsoletes: none
```

### Status of This Memo

This document proposes an experimental Jolt community and membership protocol.
It is not an IETF publication. Distribution of this memo is unlimited.

Unlike RFCs 0002 through 0004, the community records in this memo are not
implemented. Every wire shape and processing rule remains subject to review.

### Abstract

This document defines a community as an ordinary Jolt identity that publishes
policy, membership, revocation, and app-index references through generic signed
paths. It separates local interest in a community (“watch”) from a signed
membership relationship (“join”), defines open, request, invite, and closed
join policies, and specifies the verification of community-signed grants and
revocations.

Communities provide a discovery and authority boundary without making relays
owners of membership, search, or application data. Application concepts remain
above the Jolt protocol layer.

### Table of Contents

1. Introduction
2. Conventions and Requirements Language
3. Scope
4. Terminology
5. Community Identity and Paths
6. Record Model
7. Canonical Record Encoding
8. Watch and Join
9. Join Policy Processing
10. Membership Materialization
11. Member-Only State
12. Recipient Ingress
13. Error Conditions
14. Compatibility and Versioning
15. Security Considerations
16. Privacy Considerations
17. IANA Considerations
18. Implementation Status
19. References
Appendix A. Request-Policy Flow

## 1. Introduction

Jolt needs discovery structures richer than a global list of identity
addresses, but a relay-owned index would recreate the platform authority the
protocol is designed to avoid. Communities provide a signed, portable scope for
discovery and participation.

A community is not a new identity type. It is a normal `.jolt` identity whose
authorized devices publish community records. The protocol verifies identities,
device authority, generic records, CIDs, and encrypted envelopes. Applications
interpret the purpose and content of a community.

The governing split is:

```text
identity      owns the namespace
community     signs policy and membership
application   interprets indexes and interactions
relay         helps discovery and availability without authority
```

## 2. Conventions and Requirements Language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** are interpreted as BCP 14 [RFC2119] [RFC8174]. Primitive binary
encodings use JOLT-RFC-0002 Section 5.

## 3. Scope

This memo defines:

- community identities and well-known paths;
- local watch versus signed membership;
- open, request, invite, and closed join policies;
- policy, join-request, invite, membership-grant, and revocation records;
- deterministic membership state materialization;
- member-only encrypted state and prospective revocation;
- the use of recipient ingress for requests.

This memo does not define:

- global search or relay-owned indexes;
- application concepts such as posts, feeds, games, files, or moderation UI;
- ranking algorithms;
- group-key optimization;
- automatic joining of default communities;
- community-name uniqueness.

## 4. Terminology

**Community identity**
: A normal Jolt identity whose signed namespace carries community records.

**Watch**
: A local preference to retain and refresh public community state. It creates
  no protocol authority.

**Join**
: A request/decision flow ending in a valid community-signed membership grant.

**Grant**
: A community-authorized statement naming a member identity, role,
  capabilities, validity, and provenance.

**Member view**
: The materialized current membership state for one identity.

## 5. Community Identity and Paths

A community MUST use the identity, device authority, signed state, and content
rules of RFCs 0001 through 0003. No flag in an identity identifier marks it as
a community.

The following well-known paths are reserved by this draft:

```text
/.well-known/jolt/community/profile
/.well-known/jolt/community/policy
/.well-known/jolt/community/members
/.well-known/jolt/community/revocations
/.well-known/jolt/community/invites
```

Policy and profile are singleton state. Grants, revocations, and invites SHOULD
use append records so independent authorized community devices can publish
without overwriting one another.

Application-owned community state belongs under:

```text
/apps/{app_id}/public-index
/apps/{app_id}/member-index
/apps/{app_id}/submissions
```

Those paths are specified further by JOLT-RFC-0006. Core protocol code MUST NOT
interpret their payload schemas.

## 6. Record Model

### 6.1 Community Policy

```text
CommunityPolicy {
  record_type: "jolt.community_policy",
  version: 1,
  community_identity: IdentityId,
  join_policy: open | request | invite | closed,
  public_state_prefixes: [string],
  member_state_prefixes: [string],
  default_member_role: string,
  published_at: uint64
}
```

The policy object is referenced by a community-signed singleton path. Prefixes
MUST be canonical absolute Jolt paths and MUST NOT overlap in a way that labels
the same path both public and member-only.

### 6.2 Join Request

```text
CommunityJoinRequest {
  record_type: "jolt.community_join_request",
  version: 1,
  community_identity: IdentityId,
  requester_identity: IdentityId,
  requested_role: string,
  request_id: string,
  message_cid: optional ContentId,
  created_at: uint64,
  requester_public_key: bytes[32],
  requester_signature: bytes[64]
}
```

The request ID MUST be stable for retry deduplication. A request is a claim,
not membership.

### 6.3 Invite

```text
CommunityInvite {
  record_type: "jolt.community_invite",
  version: 1,
  community_identity: IdentityId,
  invite_id: string,
  invited_identity: optional IdentityId,
  role: string,
  expires_at: optional uint64,
  created_at: uint64
}
```

An invite is valid only when referenced by community-signed state. Acceptance
still requires a grant.

### 6.4 Membership Grant

```text
CommunityMembershipGrant {
  record_type: "jolt.community_membership_grant",
  version: 1,
  community_identity: IdentityId,
  member_identity: IdentityId,
  grant_id: string,
  role: string,
  capabilities: [string],
  accepted_request_id: optional string,
  invite_id: optional string,
  granted_at: uint64,
  expires_at: optional uint64
}
```

The community signature proves curation and authority to grant. It does not
make the community the author of later member-submitted application objects.

### 6.5 Membership Revocation

```text
CommunityMembershipRevocation {
  record_type: "jolt.community_membership_revocation",
  version: 1,
  community_identity: IdentityId,
  grant_id: optional string,
  member_identity: IdentityId,
  reason: optional string,
  revoked_at: uint64
}
```

A revocation MAY target one grant or all current grants for the named member.
This choice MUST be explicit during review before the RFC can be accepted.

## 7. Canonical Record Encoding

Each record body begins with its record-type-specific ASCII domain followed by
NUL, then encodes fields in the order shown in Section 6 using the binary
primitives from JOLT-RFC-0002.

```text
jolt:community-policy:v1\0
jolt:community-join-request:v1\0
jolt:community-invite:v1\0
jolt:community-membership-grant:v1\0
jolt:community-membership-revocation:v1\0
```

Enum values are encoded as one octet in the order listed, beginning at zero.
Roles and capability labels are signed strings and are compared byte-for-byte.

Join requests are signed by `requester_public_key`, which MUST derive
`requester_identity`. Policy, invite, grant, and revocation authority is
provided by the community's verified device-writer entry that binds the object
CID. A later revision may add portable inner community-device signatures if
objects must verify without their path proof.

This canonical encoding is a draft compatibility proposal and has no current
implementation.

## 8. Watch and Join

Watching is stored locally. It MUST NOT be published as a membership claim,
grant access to member paths, or cause a user to be silently joined.

Joining has the state progression:

```text
none -> pending -> active -> revoked
                -> rejected
                -> expired
```

Only a verified, unexpired grant produces `active`. A pending request, valid
invite, local watch, or cached member index does not.

## 9. Join Policy Processing

For `open`, a community service MAY automatically validate a request and
publish a grant. The resulting grant remains the source of truth.

For `request`, an authorized community device decides whether to publish a
grant. Rejection MAY be local or published as a future response record; no
rejection wire record is fixed by v1.

For `invite`, a grant MUST reference a valid, unexpired invite that either names
the requester or is valid under a future opaque-token rule. Opaque bearer invite
tokens remain unresolved in this draft.

For `closed`, unsolicited requests MUST NOT produce automatic grants.

## 10. Membership Materialization

To compute membership, a resolver MUST:

1. verify community identity authority under JOLT-RFC-0002;
2. resolve and verify policy, grant, invite, and revocation path state;
3. verify each record type/version and identity binding;
4. require the publishing community device to have been authorized at the
   corresponding writer sequence;
5. select grants whose member identity equals the subject identity;
6. reject grants inconsistent with the policy under which they were created;
7. remove expired grants;
8. apply valid revocations after their target grants;
9. return state, role, generic capabilities, grant ID, and diagnostics.

Discovery order MUST NOT change the materialized result. If conflicting valid
grants assign different roles, v1 SHOULD select the greatest grant writer tuple
from JOLT-RFC-0003 while retaining other grants as diagnostics. This rule is
open for review.

## 11. Member-Only State

Public community state is ordinary signed content that any node may fetch and
cache. Member-only state MUST be an encrypted object or encrypted index under
JOLT-RFC-0004.

For new member-only objects, recipient wraps are formed from current active
members' authorized device encryption keys. A new grant affects future wraps.
A revocation excludes the member from future wraps. Historical access requires
rewrap and cannot revoke plaintext already learned.

## 12. Recipient Ingress

Join requests are delivered to the community identity using generic recipient
ingress. Ingress provides transport and pending-queue semantics only. The
receiver MUST verify the requester signature and policy before acting. An
ingress acceptance action is not a membership grant.

## 13. Error Conditions

Implementations SHOULD distinguish malformed record, unsupported version,
identity mismatch, invalid requester signature, invalid community path proof,
unknown policy, disallowed request, expired invite, invite subject mismatch,
expired grant, unknown revocation target, and conflicting grant.

Malformed or unavailable community records MUST fail closed for member-only
authority while still allowing public watch behavior.

## 14. Compatibility and Versioning

Community records are versioned independently of core signed paths. Unknown
record versions MUST NOT be interpreted as v1. Application indexes may evolve
without changing this RFC because their schemas remain application-owned.

A future group-key mechanism must preserve the v1 prospective revocation truth
and requires its own compatibility specification.

## 15. Security Considerations

Relays, caches, and ingress senders are untrusted. Community authority comes
from verified identity/device signatures and grants, never from where bytes
were found.

Open auto-join endpoints require abuse controls, deduplication, size limits,
and rate limits. Role and capability labels MUST NOT grant daemon authority
outside explicitly defined community operations.

Community device compromise permits fraudulent grants until that device is
revoked. Resolvers MUST apply device authority at the relevant writer sequence.

## 16. Privacy Considerations

Public grants reveal community association, roles, timing, and revocations.
Private communities SHOULD encrypt membership collections when public
membership is undesirable, but outer path/CID and traffic metadata remain.

Join messages SHOULD avoid unnecessary personal data. Watching remains local
specifically so interest in public communities need not be published.

## 17. IANA Considerations

This memo requests no IANA actions. Paths, domains, and record names are
project-local experimental identifiers.

## 18. Implementation Status

No community policy, membership, invite, grant, revocation, watch, or join
record is implemented. The identity authority, writer-log, encryption, and
ingress foundations exist. This design is derived from architecture document 21
and cards 098 and 099.

Unresolved questions include opaque invites, grant-target revocation semantics,
conflicting role selection, published rejection records, and portable inner
community signatures. The RFC MUST NOT advance to Accepted until those
questions are decided.

## 19. References

### 19.1 Normative References

- JOLT-RFC-0001, “Signed Path Records and Resolution.”
- JOLT-RFC-0002, “Device Authorization and Revocation.”
- JOLT-RFC-0003, “Per-Device Writer Logs and Deterministic Merge.”
- JOLT-RFC-0004, “Encrypted Objects and Private Device Access.”

### 19.2 Informative References

- Jolt document 21, “Community Identity and Membership Model.”
- Jolt card 098, “Community Identity and Membership Model.”

## Appendix A. Request-Policy Flow

For a `request` community, Alice signs a join request and sends it through
recipient ingress. A community admin verifies Alice, current policy, and request
deduplication. Acceptance publishes a community-signed grant append record.
Alice is not active until her resolver obtains and verifies that grant. Later
revocation publishes a separate community-signed record and excludes Alice from
future member-only encryption wraps.

