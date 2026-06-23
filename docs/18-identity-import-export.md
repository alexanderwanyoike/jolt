# Identity Import and Export v0

## Status

Implemented as the v0 recovery bridge in card `096a`.

The shipped v0 exposes admin-only daemon API routes, CLI commands, and Console
controls for encrypted identity export/import. It protects the export bundle
with a passphrase-derived Argon2id/XChaCha20-Poly1305 key and stores imported
material through the normal daemon identity and local identity encryption-key
paths.

This remains shared-key portability, not delegated multi-device authorization:
an imported daemon has the same root identity and peer key as the source daemon,
app sessions are not imported, and users must approve apps again after import.

## Problem

Jolt identities are long-lived Ed25519 signing keys. The identity key is the
authority behind a `.jolt` address, update-log entries, home-relay pin requests,
and signed encryption-key records.

Users need a way to carry the same identity to a new daemon or recover after a
device loss. The simplest v0 is key import/export: copy the same identity
authority onto another daemon.

That is useful, but it is dangerous. Every imported device can fully impersonate
the identity. There is no per-device revocation until Jolt has delegated device
keys.

## Goals

- Let a user export enough key material to recover the same `.jolt` identity.
- Let a user import that bundle into a new local daemon.
- Keep export/import admin-only. Normal app sessions must never receive this
  authority.
- Make the v0 risks explicit before implementation.
- Leave a clear path toward delegated device keys.

## Non-Goals

- No app-facing key export/import capability.
- No cloud backup service.
- No social recovery.
- No multi-device conflict resolution.
- No per-device revocation in v0.
- No migration to delegated device keys in this card.
- No guarantee that cached content, app data, or unpublished local drafts move
  with an identity export.

## Authority Model

An identity export is not an app data export. It is a recovery bundle for local
daemon authority.

The v0 export must include:

- Root identity signing secret for the `.jolt` identity.
- Public identity key and derived identity ID for self-checking.
- Local identity encryption private keys needed to decrypt existing private
  objects addressed to this identity.
- Public metadata for those encryption keys, such as key IDs, suite IDs, and
  creation/validity times.
- Export metadata: format version, creation time, source daemon version if
  available, and optional human label.

The v0 export should not include:

- App session tokens.
- Approved app grants.
- Relay cache entries.
- Content cache bytes.
- Debug logs.
- Plaintext private content.
- Home-relay secrets, if future versions add any.

Imported devices may fetch public content, signed update logs, and encrypted
objects from the network after import. If the network cannot provide old content
or old update-log snapshots, the imported device may have the key but not the
bytes.

## Export File Format

The exported file should be a single encrypted JSON envelope with a stable magic
string and version:

```json
{
  "magic": "jolt.identity.export",
  "version": 1,
  "kdf": {
    "name": "argon2id",
    "params": {
      "memory_kib": 65536,
      "iterations": 3,
      "parallelism": 1
    },
    "salt": "base64url..."
  },
  "cipher": {
    "name": "xchacha20poly1305",
    "nonce": "base64url..."
  },
  "created_at": "2026-06-06T00:00:00Z",
  "identity": "lgth...xwa.jolt",
  "ciphertext": "base64url..."
}
```

The decrypted plaintext should be canonical JSON:

```json
{
  "type": "jolt.identity.export.plaintext",
  "version": 1,
  "identity": {
    "id": "lgth...xwa",
    "address": "lgth...xwa.jolt",
    "public_key_ed25519": "base64url...",
    "secret_key_ed25519": "base64url..."
  },
  "identity_encryption_keys": [
    {
      "suite": "enc_x25519_v1",
      "key_id": "enc_x25519_local_v0",
      "public_key": "base64url...",
      "private_key": "base64url...",
      "created_at": 1780000000,
      "not_before": 1780000000,
      "not_after": null
    }
  ],
  "source": {
    "jolt_version": "optional",
    "exported_at": "2026-06-06T00:00:00Z",
    "label": "optional human label"
  }
}
```

Implementation may encode the same fields with a binary container later, but v0
should prefer explicit JSON for reviewability and migration.

The outer `identity` field is public metadata for operator sanity checks. It
must match the decrypted public key and derived identity ID. Import must reject a
bundle where those values disagree.

## Protection

Export must require an export passphrase. It must not silently reuse an app
session token, daemon admin token, or OS username.

The KDF and cipher choices should match the at-rest key storage direction:

- Argon2id for deriving an export key from the passphrase.
- XChaCha20-Poly1305 for authenticated encryption.
- Random salt and nonce per export.
- Associated data should include `magic`, `version`, `identity`, KDF name, and
  cipher name so header tampering is detected.

The export command should refuse weak or empty passphrases in normal builds. A
test/dev override can exist only behind an explicit flag.

The export file is still a bearer secret. Anyone with the file and passphrase can
become that identity.

## Admin Surface

Export/import belongs to the admin/Console trust class only.

Suggested CLI:

```text
jolt identity export --out alice.jolt-identity --label "Alice laptop backup"
jolt identity import --from alice.jolt-identity
```

Suggested local admin API:

```text
POST /admin/v1/identities/export
POST /admin/v1/identities/import
```

Normal app sessions must not be able to request equivalent capabilities. The
existing app boundary already treats `export:keys` and `delete:identity` as
forbidden normal-app capabilities; identity export/import must stay in that
same forbidden category.

Console UX should require an explicit confirmation step that says:

```text
This exports the private keys for this .jolt identity. Anyone who imports this
file and knows its passphrase can act as this identity. Jolt v0 cannot revoke
one copied device without rotating away from this identity.
```

## Import Rules

Import should be conservative:

- Import into an empty daemon profile by default.
- Refuse to overwrite an existing local identity unless the user passes an
  explicit replacement flag and confirms in Console.
- Verify the decrypted Ed25519 public key derives the expected identity ID.
- Verify each exported identity encryption private key matches its public key.
- Store imported key material using the local daemon's normal key-storage path.
- Do not import app sessions from the old device.
- Do not auto-approve apps on the new device.
- Publish or republish the identity encryption-key record after import if the
  daemon has network access.

If the imported bundle contains no identity encryption private keys, the daemon
can still sign as the identity, but it must warn that old private objects may be
undecryptable from this device.

## Update-Log and Multi-Device Risks

With v0 shared-key import, every imported daemon has the same signing authority.
That creates three major risks:

- **Compromise blast radius:** compromising any copied device compromises the
  whole identity.
- **No per-device revocation:** Jolt cannot revoke just the lost laptop. The
  user must migrate to a new identity or a future delegated-key model.
- **Concurrent update-log conflicts:** two online devices can both append from
  the same latest sequence, producing competing valid histories.

The first implementation should treat import as recovery or deliberate device
move, not seamless multi-device collaboration. It should warn users before they
keep the same identity active on multiple devices.

For v0, a daemon that detects a local update-log head conflict for its own
identity should stop publishing and require user/admin intervention rather than
silently choosing one branch.

## Interaction With Current Storage

The current implementation stores the identity signing key in `keypair.bin` with
restrictive file permissions and still has a TODO for encrypted-at-rest key
storage. The design docs describe encrypted at-rest storage as the target model.

Before shipping user-facing identity export/import, implementation should either:

- finish encrypted-at-rest local key storage first, or
- make it explicit that export encryption protects only the exported file, not
  the daemon's existing local key file.

The import/export format should not depend on the current `keypair.bin` filename
or raw on-disk representation.

## Future: Delegated Device Keys

The safer long-term model is:

```text
root identity key
  signs device delegation records
    device key A can publish under /pastes/*
    device key B can publish under /profile
    device key C can decrypt with encryption key K
```

In that model:

- The root identity key can be kept offline or rarely used.
- Each device has its own signing key.
- Device grants are scoped by path, capability, expiry, and sequence window.
- A compromised device can be revoked by publishing a signed revocation record.
- Update-log entries can identify which delegated device signed them.
- Conflict handling can use delegated-authority metadata instead of pretending
  all writes came from one physical device.

This is future work. V0 key import/export should avoid naming or formatting that
would block this migration.

## Implementation Slices

1. Define export/import core types and round-trip tests.
2. Add encrypted export writer and reader with passphrase-based protection.
3. Add identity import validation against public key, identity ID, and
   encryption-key pairs.
4. Add CLI/admin export/import commands.
5. Add Console confirmation UX.
6. Add own-update-log conflict detection before advertising multi-device use.

## Open Questions

- Should v0 export include the local update-log snapshot for offline recovery,
  or should it stay strictly key material?
- Should import support adding a second identity to a daemon, or only replacing
  an empty profile?
- Should Console require the user to type the identity suffix before export, as
  a stronger confirmation?
