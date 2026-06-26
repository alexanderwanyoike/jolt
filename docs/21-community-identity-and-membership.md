# Community Identity and Membership Model

## Goal

Communities are Jolt identities. They are not a separate protocol namespace and
they are not relay-owned search indexes.

The product rule is:

```text
apps give purpose;
communities give discovery;
identities give ownership;
Jolt gives signed state, transport, encryption, and availability.
```

A community identity owns signed state under normal Jolt paths. Apps can read
that state and interpret app-specific indexes, but protocol code only sees
identities, signed records, CIDs, encrypted envelopes, and generic paths.

## Community Identity

A community identity is a normal Jolt identity with its own device authority.
Its authorized devices/admins publish community state through the existing
identity path model.

Recommended well-known paths:

```text
/.well-known/jolt/community/profile
/.well-known/jolt/community/policy
/.well-known/jolt/community/members
/.well-known/jolt/community/revocations
/.well-known/jolt/community/invites
```

App-specific community state lives above that layer:

```text
/apps/{app_id}/public-index
/apps/{app_id}/member-index
/apps/{app_id}/submissions
```

Jolt does not know whether an app index is a Spoke feed, file catalog, paste
collection, release list, game lobby, or anything else. It only verifies that a
community identity signed the path binding and that submitted member records
carry valid member signatures.

## Watch Versus Join

Watching is local interest in public community state. Joining is a signed
relationship with the community.

```text
watch = local setting: keep this community visible and refresh public state
join = community-signed membership grant
```

A watched community can be shown in Console or apps without the user being a
member. Watching does not grant posting, member-only reads, or decryption.

Local watch records are device-local preferences. They are not membership
claims and do not need to be published as protocol state. A later card may sync
watch lists across a user's devices as app/user preference data, but that is not
part of community authority.

## Join Policy

The community policy record defines how a non-member may become a member.

Policies:

- `open`: anyone may request membership and should receive a community-signed
  grant automatically.
- `request`: anyone may submit a join request, but an authorized community
  admin/device must accept or reject it.
- `invite`: users can join only by presenting a valid community-signed invite.
- `closed`: public join requests are not accepted.

Even open join produces a signed membership grant. The grant is the source of
truth for later role checks, member-only index access, and revocation.

## Signed Records

### Community Policy

The policy record is signed by an authorized community device.

```text
record_type: jolt.community_policy
version: 1
community_identity: <identity>
join_policy: open | request | invite | closed
public_state: <list of public path prefixes>
member_state: <list of member-only path prefixes>
default_member_role: member
published_at: <logical time>
```

This record is generic. App-specific rules, categories, moderation labels, or
feed policies belong in app-owned schemas referenced from community app indexes.

### Join Request

A join request is signed by the requester identity and delivered to the
community identity through recipient ingress.

```text
record_type: jolt.community_join_request
version: 1
community_identity: <identity>
requester_identity: <identity>
requested_role: member
request_id: <stable id>
message_cid: <optional public or encrypted note>
created_at: <logical time>
requester_signature: <signature>
```

Recipient ingress is the transport. It is not the authority. The community
accepts a request only by publishing a community-signed grant.

For an `open` community, the daemon may auto-accept a valid request and publish
the grant. For `request`, the request remains pending until an authorized
community admin/device decides. For `invite` and `closed`, unsolicited requests
can be rejected or ignored.

### Invite

An invite is signed by the community and names the invited identity, or carries
an opaque invite token for a future flow.

```text
record_type: jolt.community_invite
version: 1
community_identity: <identity>
invite_id: <stable id>
invited_identity: <optional identity>
role: member
expires_at: <optional>
created_at: <logical time>
community_signature: <signature>
```

Invite acceptance still results in a membership grant. Apps and daemons should
check grants, not invites, when deciding current membership.

### Membership Grant

A grant is signed by an authorized community device and published under the
community identity.

```text
record_type: jolt.community_membership_grant
version: 1
community_identity: <identity>
member_identity: <identity>
grant_id: <stable id>
role: member | moderator | admin
capabilities: <generic community capabilities>
accepted_request_id: <optional request id>
invite_id: <optional invite id>
granted_at: <logical time>
expires_at: <optional>
community_signature: <signature>
```

Roles and capabilities are generic community authority. App-specific posting or
moderation semantics must be expressed in app schemas above the protocol layer.

### Membership Revocation

A revocation is signed by an authorized community device and names a grant or
member identity.

```text
record_type: jolt.community_membership_revocation
version: 1
community_identity: <identity>
grant_id: <optional grant id>
member_identity: <identity>
reason: <optional string>
revoked_at: <logical time>
community_signature: <signature>
```

Revocation prevents future accepted member actions and future encryption
wrapping. It cannot claw back plaintext that a member already decrypted.

## Verification Rules

To verify membership for a community:

1. Resolve and verify the community identity's device authority.
2. Fetch the community policy, grants, and revocations from community-owned
   paths.
3. Verify that each policy, grant, invite, and revocation was signed by an
   authorized community device at the relevant sequence.
4. Verify the member identity in a grant matches the session identity being
   checked.
5. Reject expired grants.
6. Apply revocations after grants.
7. Return a materialized member view: `none`, `pending`, `active`, `revoked`,
   or `expired`, plus role and generic capabilities when active.

Join requests are requester claims until accepted. They do not create
membership.

## Public And Member-Only State

Public community state is ordinary signed content under the community identity.
Anyone can watch it, fetch it, cache it, and render it.

Member-only state is encrypted content or encrypted app indexes. The community
wraps keys for accepted members' authorized device encryption keys. This uses
the same private-content rule as user-device private data:

```text
new grants affect future wraps;
revocation protects future writes;
historical plaintext cannot be clawed back;
historical encrypted state needs rewrap if newly accepted members should read it.
```

For v0, member-only encryption can be implemented by wrapping encrypted object
keys directly for accepted member devices. Group-key optimization can come
later, but it must preserve the same revocation truth: removed members cannot
decrypt future member-only state.

## App Assumptions

An app may assume only generic facts from Jolt membership APIs:

- this session identity is watching a community locally;
- this session identity has a pending request;
- this session identity has an active, expired, rejected, or revoked grant;
- active grants may carry generic role/capability labels;
- community app indexes are signed by the community identity;
- member submissions remain signed by the member identity.

An app may not assume that Jolt protocol code understands posts, feeds, files,
galleries, games, timelines, or moderation semantics. Those belong in app
schemas and app-specific indexes.

## Default Discoverable Communities

Default communities are local discovery hints, not authority.

A fresh install may ship a configurable list of community identities. The user
can watch, hide, remove, or join them through normal policy. The user is never
silently joined.

Defaults should be replaceable for demo/development builds without becoming a
protocol dependency.

## Discovery Without Relay-Owned Search

Communities provide discovery by publishing signed app indexes that clients can
fetch and search locally.

```text
discover community identity -> watch public state -> fetch signed app indexes
-> search locally -> fetch selected CIDs
```

Relays can make community state available, but they do not decide search
results. A relay may serve bytes, provider records, and pinned update logs. It
does not own truth for membership, app indexes, or ranking.

## Implementation Slices

Card 099 should implement the generic membership path:

1. Local watch records for community identities.
2. Community policy publishing.
3. Signed join requests through recipient ingress.
4. Open auto-join with community-signed grants.
5. Request accept/reject by authorized community admin/device.
6. Membership revocation.
7. App-visible membership state.

Card 100 should then build community-scoped app indexes on top of verified
membership without adding app concepts to protocol code.
