# 096a: Identity Export/Import and Console Recovery v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 096  
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

- [ ] `jolt identity export --out <file>` writes a passphrase-protected identity
      recovery bundle.
- [ ] `jolt identity import --from <file>` imports the bundle into a daemon
      profile after validating the decrypted identity material.
- [ ] The admin API exposes export/import routes that are unavailable to normal
      app sessions.
- [ ] Console can export the active identity after a clear private-key risk
      confirmation.
- [ ] Console can import an identity bundle into a daemon profile without
      silently overwriting an existing identity.
- [ ] The export includes signing key material and local identity encryption
      private keys needed for existing private objects.
- [ ] Imported identities do not import app sessions and do not auto-approve
      apps.
- [ ] A two-daemon smoke path proves an imported identity can approve Spoke or a
      test app and resolve the same identity-scoped app index.
- [ ] Docs explain that v0 export/import is recovery/shared-key portability, not
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
