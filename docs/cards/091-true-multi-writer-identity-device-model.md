# 091: True Multi-Writer Identity and Device Model

**Type:** HITL  
**Milestone:** Identity and Device Sprint  
**Status:** Discussion next  
**Blocked by:** None

## Why

Jolt has treated an identity mostly as a local signing key. That is enough for
single-device demos, but it is not enough for real use.

Users need to use the same identity from multiple devices without copying the
same private writer key everywhere. Apps also need identity-scoped authority, so
the identity/device boundary has to be clear before Spoke, Pastey, or any future
Jolt browser can feel safe.

The target model is true multi-writer from the start:

```text
user identity = durable namespace
device identity = authorized writer for that namespace
app session = scoped grant on one device for one user identity
```

## What to Decide

- Define the signed records that make a device an authorized writer for a user
  identity.
- Define how a device is revoked.
- Define whether the initial local identity key is a root key, first device key,
  or transitional combined key.
- Define how authorized device logs are discovered.
- Define the deterministic merge rules for multiple device writer logs.
- Define conflict behavior for singleton paths such as `/profile`.
- Define append-style behavior for app feeds, posts, pastes, and replies.
- Define how identity state sync differs from content byte sync.
- Define how encrypted app indexes and encrypted referenced content follow an
  identity across authorized devices.
- Define how private content access should work for newly authorized devices.

## Acceptance Criteria

- [ ] The model allows multiple devices to write for one user identity without
      sharing one private writer key.
- [ ] Device authorization and revocation are represented as signed,
      verifiable identity state.
- [ ] The design preserves the protocol boundary: no protocol-level profiles,
      posts, feeds, pastes, galleries, or browser concepts.
- [ ] The design distinguishes control-plane sync from content fetch/cache/pin.
- [ ] The design supports encrypted app data, including private app indexes and
      encrypted content bodies.
- [ ] The design states what happens when two devices update the same path.
- [ ] The design states what happens when two devices publish append-style app
      records concurrently.
- [ ] The design states how a new device gains access to future and historical
      private content.
- [ ] The design names the minimum migration path from today's single-writer
      identity.

## Non-Goals

- Jolt browser.
- Storage markets or payment mechanics.
- A full CRDT framework for all application data.
- Automatic byte-level sync of every CID an identity has ever referenced.

## Notes

The key product rule is:

```text
identity state follows automatically;
app indexes follow automatically;
content follows by fetch/cache/pin policy;
private content follows by encryption grants.
```
