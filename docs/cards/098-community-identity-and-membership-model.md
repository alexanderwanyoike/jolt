# 098: Community Identity and Membership Model

**Type:** HITL
**Milestone:** Community Discovery Sprint
**Status:** Ready after 091
**Blocked by:** 091

## Why

Jolt needs discovery that is better than manually knowing identities, but search
should not become a relay-owned platform service.

The working model is:

```text
apps give purpose;
communities give discovery;
identities give ownership;
Jolt gives signed state, transport, encryption, and availability.
```

A community should be a Jolt identity. It can publish signed policy,
membership, and app indexes without Jolt protocol code understanding Spoke
feeds, file libraries, paste collections, or other app concepts.

## What to Decide

- Define a community as a Jolt identity with community-owned signed paths.
- Define local watch/subscribe versus membership join.
- Define join policies:
  - open;
  - request;
  - invite;
  - closed.
- Define the signed records for membership grants and revocations.
- Define how generic recipient ingress carries join requests.
- Define how public community state differs from member-only encrypted state.
- Define what apps can assume from a community membership grant.
- Define how default discoverable communities work without auto-joining users.

## Acceptance Criteria

- [ ] The design treats communities as Jolt identities, not a separate protocol
      namespace.
- [ ] Users can watch public community state without joining.
- [ ] Users can request or automatically obtain membership depending on the
      community join policy.
- [ ] Community membership and revocation are signed and verifiable.
- [ ] Member-only community state can be encrypted to accepted members/devices.
- [ ] The design keeps app semantics above the protocol layer.
- [ ] The design explains how communities enable discovery without relay-owned
      search.

## Non-Goals

- Global search.
- Relay-owned search indexes.
- App store/catalog mechanics.
- Protocol-level Spoke, Pastey, or file-sharing semantics.

## Notes

The key split is:

```text
watch = local interest in public community state
join = signed relationship granting participation and possibly decryption
```
