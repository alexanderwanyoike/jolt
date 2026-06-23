# 096a: Identity Export/Import and Console Recovery v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Implemented in PR
**Blocked by:** 048, 096

## Why

Card 096 makes app indexes follow an identity, but users still need an honest way
to carry that identity to another daemon or recover it on another machine.

For v0, Jolt should support deliberate, admin-only identity recovery:

```text
export identity from Console or CLI;
protect the export with a passphrase;
import it into another daemon profile;
approve apps again on that device;
see identity-scoped app data resolve through the normal app/index path.
```

This is not the final multi-device security model. Copying the root identity key
means every imported daemon can fully act as that identity. The product must say
that plainly while still giving users a usable recovery path.

## What to Build

Implement the card 048 design as a user-facing recovery flow:

- encrypted identity export bundle format;
- CLI export/import commands;
- admin-only daemon API for export/import;
- Console export/import UX with explicit risk confirmation;
- import validation against public key, derived identity ID, and encryption key
  material;
- storage through the normal local identity/key paths, not ad hoc file copying;
- no app-session export/import;
- no automatic app approval after import;
- smoke verification that an imported identity can approve an app and resolve
  identity-scoped app data.

## Acceptance Criteria

- [x] `jolt identity export --out <file>` writes a passphrase-protected identity
      recovery bundle.
- [x] `jolt identity import --from <file>` imports the bundle into a daemon
      profile after validating the decrypted identity material.
- [x] The admin API exposes export/import routes that are unavailable to normal
      app sessions.
- [x] Console can export the active identity after a clear private-key risk
      confirmation.
- [x] Console can import an identity bundle into a daemon profile without
      silently overwriting an existing identity.
- [x] The export includes signing key material and local identity encryption
      private keys needed for existing private objects.
- [x] Imported identities do not import app sessions and do not auto-approve
      apps.
- [x] A two-profile smoke path proves an imported identity can approve a test app
      after daemon restart and publish/enumerate the same identity-scoped app
      index path.
- [x] Docs explain that v0 export/import is recovery/shared-key portability, not
      revocable delegated device authorization.

## Non-Goals

- Cloud backup.
- Normal app capability to export keys.
- Silent background identity sync.
- Solving per-device root-key revocation.
- Historical private-content rewrap for newly authorized devices; that remains
  card 097.

## Notes

This card should preserve the safety language from
[Identity Import and Export v0](../18-identity-import-export.md):

```text
Anyone with the export file and passphrase can become this identity.
```

The safer long-term model is delegated device authorization, where a new device
gets its own revocable writer/decrypt authority instead of receiving the root
identity key. This card is the pragmatic recovery bridge needed before app data
following can feel real to users.

## Verification

- Red: `cargo test -p jolt-identity export_bundle -- --nocapture` failed before
  `NodeIdentity` exposed signing-key import/export helpers.
- Red: `cargo test -p jolt-server test_admin_can_export_and_import_identity_recovery_bundle --test api_integration -- --nocapture`
  failed before the identity recovery API was re-exported and wired.
- Green: `cargo test -p jolt-identity export_bundle -- --nocapture`
- Green: `cargo test -p jolt-node parse_identity_export_import_commands --bin jolt -- --nocapture`
- Green: `cargo test -p jolt-server test_admin_can_export_and_import_identity_recovery_bundle --test api_integration -- --nocapture`
- Green: `npx vitest run src/daemon/client.test.ts src/sections/sections.test.tsx`
  from `apps/jolt-console`
- Green: `npm run build` from `apps/jolt-console`
- Green: `./scripts/test-local.sh`

The integration smoke uses two daemon profiles: the source exports an encrypted
bundle, the target refuses overwrite without explicit permission, imports with
overwrite, restarts, proves app sessions were not imported, approves a fresh app
session, and publishes/enumerates an app append record under the imported
identity. It does not assert that two concurrently running cloned root-key
daemons sync with each other, because this v0 recovery model intentionally
copies the root peer/identity key.
