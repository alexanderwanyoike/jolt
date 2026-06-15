# True Multi-Writer Identity and Devices

## Status

Design proposal for card `091`.

This document updates Jolt's identity model from a single long-lived signing key
to a user identity with authorized, revocable device writers. It is a design
document, not an implementation record.

## Problem

The current model treats a Jolt identity as one Ed25519 signing key. Copying
that key to another machine lets the new machine act as the identity, but it
also creates three bad properties:

- every copied device has full root authority;
- a lost device cannot be revoked independently;
- two online devices can append conflicting update-log histories.

Jolt needs true multi-writer identity from the start of the next product phase.
Users should be able to use the same user identity from multiple devices without
copying one private writer key everywhere.

## Design Goals

- A user identity is a durable namespace, not a device.
- Each device has its own signing and encryption key material.
- Devices can be authorized and revoked.
- Multiple authorized devices can publish concurrently.
- Resolution is deterministic regardless of device-log discovery order.
- Apps remain scoped to one app, one local device, and one user identity.
- Control-plane state sync is distinct from content byte sync.
- Private content and private app indexes remain encrypted for authorized
  devices.
- Protocol code stays application-agnostic.

## Non-Goals

- Generic CRDT support for app documents.
- Protocol-level profiles, feeds, posts, pastes, galleries, threads, or
  communities.
- Global search.
- Remote deletion of data already cached or decrypted by a revoked device.
- Social recovery.
- Hiding all metadata, such as path names, object sizes, CIDs, timing, or
  recipient counts.

## Core Model

```text
user identity
  durable namespace and root authority

device identity
  authorized writer for one user identity

device writer log
  append-only signed operations from one authorized device

merged identity state
  deterministic materialized view over authorized device logs

app session
  local grant for one app, one device, one user identity, and capabilities
```

The user identity remains the addressable `.jolt` identity. Devices are not new
human-facing identities. They are delegated authorities under the user identity.

## Key Roles

Jolt should separate four key roles:

- **Root identity signing key:** authorizes devices, revokes devices, and signs
  root authority records.
- **Device signing key:** signs one device's writer log entries.
- **Device encryption key:** receives wraps for private app indexes and private
  content intended for that device.
- **Content encryption key:** one-time symmetric key for an encrypted object or
  encrypted app index.

The current Ed25519 identity key can act as the transitional root key and first
device key during migration, but the target model should avoid requiring that
same secret on every device.

## Authority Records

Each user identity publishes signed authority records. These records define the
authorized device set and device capabilities.

Logical shape:

```json
{
  "type": "jolt.identity_authority_record",
  "version": 1,
  "identity": "alice.jolt",
  "sequence": 12,
  "previous": "cid-or-hash-of-previous-authority-record",
  "operation": {
    "kind": "authorize_device",
    "device_id": "dev_...",
    "device_signing_key": {
      "alg": "Ed25519",
      "public_key_b64u": "..."
    },
    "device_encryption_keys": [
      {
        "key_id": "dev_enc_2026_06",
        "suite_family": "x25519-hkdf-sha256",
        "public_key_b64u": "..."
      }
    ],
    "capabilities": [
      "identity:write",
      "app:grant",
      "encrypt:receive"
    ],
    "label": "Alice laptop",
    "created_at": 1780579200
  },
  "signature": "root-or-authorized-admin signature"
}
```

Revocation records should name the revoked device and the last accepted device
log sequence if known:

```json
{
  "type": "jolt.identity_authority_record",
  "version": 1,
  "identity": "alice.jolt",
  "sequence": 13,
  "previous": "cid-or-hash-of-previous-authority-record",
  "operation": {
    "kind": "revoke_device",
    "device_id": "dev_...",
    "accepted_through_device_sequence": 42,
    "reason": "lost_device",
    "created_at": 1780579300
  },
  "signature": "root-or-authorized-admin signature"
}
```

The `accepted_through_device_sequence` field matters because revocation cannot
make a device forget its private key. Once a revocation is published, resolvers
must reject that device's entries after the accepted sequence. If the revoker
does not know a safe latest sequence, it may set the accepted sequence lower and
force later entries into conflict review.

## Authority to Authorize Devices

The root identity key is the highest authority. A device may also be granted an
admin capability that lets it authorize or revoke other devices.

The authority chain is deliberately lower throughput than app publishing. It
does not need to be the multi-writer surface for routine posts, app indexes, or
content references. True multi-writer publishing happens through authorized
device writer logs; the authority chain only changes who may write.

For v1, prefer this conservative policy:

- root key can authorize and revoke devices;
- an admin device can authorize non-admin devices;
- root key is required to authorize another admin device;
- root key is required to rotate root authority;
- all device authorization and revocation records are part of the same identity
  authority chain.

This keeps normal multi-device use practical while avoiding a casual app or
ordinary phone becoming root-equivalent by accident.

## Device Writer Logs

Each authorized device maintains its own append-only writer log.

Logical entry shape:

```json
{
  "type": "jolt.device_writer_log_entry",
  "version": 1,
  "identity": "alice.jolt",
  "device_id": "dev_...",
  "device_sequence": 43,
  "previous": "cid-or-hash-of-previous-device-entry",
  "authority_head": "cid-or-hash-of-authority-record-known-at-write-time",
  "operation": {
    "kind": "set_path",
    "path": "/apps/pastey/index",
    "content_id": "cid...",
    "mode": "singleton"
  },
  "created_at": 1780579400,
  "signature": "device signature"
}
```

Device logs are discovered through signed device records, provider discovery,
identity-head hints, and cached local state. Discovery order must not affect the
final merged state.

## Operation Classes

Jolt should distinguish generic operation classes without understanding app
schemas.

### Singleton Paths

Singleton paths represent the current value for a path:

```text
/profile
/apps/pastey/index
/apps/spoke/profile
```

If two devices set the same singleton path concurrently, resolution picks a
deterministic winner and keeps the losing entries as conflict history for
diagnostics or app-level recovery.

Recommended ordering:

```text
(logical_time, device_sequence, device_id, entry_hash)
```

The highest tuple wins for singleton paths. Ties are impossible unless two
records have the same entry hash, in which case they are the same record.

`logical_time` should be derived from the device log's monotonic clock or
Lamport-style counter, not trusted as a wall-clock security boundary. Wall-clock
timestamps are useful for display, not authority.

### Append Records

Append-style operations represent independent app records:

```text
post
reply
paste version
community entry
membership request
```

Multiple devices can append independently. The merged state includes all valid
records in deterministic order. Applications interpret the records and decide
whether they are posts, replies, pastes, messages, or something else.

### Tombstones

Remove operations are tombstones. A tombstone should name the target entry or
path and the device entry that created it when known. Resolvers preserve enough
history to avoid reintroducing deleted records from a stale device log.

## Merged Identity State

Resolution should materialize identity state in this order:

1. Resolve and verify the identity authority chain.
2. Build the authorized device set.
3. Fetch known device writer logs.
4. Verify each device log entry:
   - signature matches the device key;
   - device was authorized;
   - entry sequence and previous hash are valid;
   - entry is not beyond an accepted revocation sequence.
5. Partition operations by generic operation class.
6. Apply singleton conflict ordering.
7. Preserve append records and tombstones in deterministic order.
8. Return the merged state plus diagnostics.

The result should be stable if the same valid records are available, regardless
of fetch order.

## Conflict Semantics

Jolt should not try to merge app-specific objects. It should only give apps a
deterministic and inspectable set of signed records.

For singleton path conflicts:

- choose a deterministic current winner;
- expose conflicts in diagnostics;
- allow an authorized device or app to publish a new winner later.

For append-style records:

- preserve all valid entries;
- let the app render, filter, moderate, or ignore records.

For revoked-device entries:

- accept entries up to the revocation's accepted device sequence;
- reject entries after that sequence;
- expose rejected entries as diagnostics if encountered locally.

## Sync Model

Identity sync is not the same as content sync.

```text
Control plane:
  authority records, device records, device log heads, path bindings,
  app-index references, membership grants

Content plane:
  CID-addressed bytes referenced by signed state

Private plane:
  encrypted app indexes, encrypted content bodies, key wraps, rewrap records
```

Control-plane state should sync automatically because it is small and required
for identity resolution.

Content bytes should be fetched lazily by default. A user or app may ask the
daemon to keep or pin selected content for offline use.

Private content requires both bytes and a valid decryption path. A device can
fetch encrypted bytes without being able to decrypt them.

## Encrypted App Indexes and Content

Private app indexes should use the same encrypted-object envelope model as
private content bodies. The identity path points to an encrypted object CID.
Only authorized devices with matching key wraps can decrypt the index.

For new private writes:

- resolve the current authorized device encryption keys;
- encrypt the object once with a content key;
- wrap the content key to the current authorized devices or intended audience;
- include the local device's own wrap;
- sign the envelope with the author/device authority required by the operation.

For newly authorized devices:

- future private objects should include a wrap for the new device;
- historical private objects are not automatically readable;
- an explicit rewrap operation can publish new envelopes or key-wrap records for
  old content.

For revoked devices:

- stop wrapping future private content to the revoked device;
- do not assume already cached or decrypted plaintext can be clawed back.

## App Session Boundary

App sessions should include:

```text
app id
local device id
user identity
capabilities
path/app scopes
expiry/revocation status
```

An app approved for identity A on device X must not silently operate on identity
B or device Y. Device revocation invalidates the device's local app sessions for
future writes.

Apps still never receive long-term private keys. Apps ask the local daemon to
sign, publish, encrypt, decrypt, fetch, pin, or resolve according to granted
capabilities.

## Migration from Current Identity Model

The minimum migration path should be incremental:

1. Treat the current Ed25519 identity key as the root identity key.
2. Create a first local device record for the current daemon.
3. In the first migration, the first device may use the same Ed25519 key as the
   legacy writer key to preserve compatibility.
4. Add support for generating a separate device signing key.
5. Publish future writes through the device writer log.
6. Continue resolving legacy update-log entries as a legacy device log until
   old records can be fully represented in the new model.

This lets existing identities keep their `.jolt` address while moving toward
revocable devices.

## Implementation Slices

The design maps to the follow-up cards:

- `092`: local multiple identities and Console identity selection.
- `093`: device authorization and revocation records.
- `094`: per-device writer logs and deterministic merge.
- `095`: identity-scoped app grants.
- `096`: app indexes and content follow-me behavior.
- `097`: private content access across authorized devices.

## Open Questions

- Should admin devices be allowed to authorize other admin devices, or should
  the root key always be required?
- Should authority records use the existing update-log mechanism during
  migration or a separate authority chain from the start?
- What is the exact canonical binary encoding for authority records and device
  log entries?
- How much conflict history should normal APIs expose versus diagnostics-only
  APIs?
- Should path names for private app indexes be obfuscated in a future privacy
  hardening pass?
