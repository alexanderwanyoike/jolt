# 095: Identity-Scoped App Grants v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 092 and 093  
**Blocked by:** 092, 093

## Why

Apps are scoped to identities, not just to the local daemon. Once a user can have
multiple local identities and devices, the app section in Console must be
identity-aware.

An app grant should answer:

```text
Which app?
On which local device?
For which user identity?
With which capabilities?
```

## What to Build

Update app session and Console grant handling so app authority is explicitly
identity-scoped:

- app approval requests name the requested user identity;
- grants are listed under the selected identity in Console;
- app APIs require the session's identity scope;
- revoking an app grant affects that app for that identity only;
- device revocation invalidates or blocks app sessions bound to that device;
- diagnostics make identity/device/app grant boundaries visible.

## Acceptance Criteria

- [ ] The same app can be approved for one local identity without being approved
      for another.
- [ ] Console shows pending, active, rejected, and revoked grants for the
      selected identity.
- [ ] App APIs cannot silently operate on the wrong local identity.
- [ ] Revoking an app grant for identity A does not revoke identity B's grant.
- [ ] Revoking a device prevents that device's app sessions from continuing to
      write as the user identity.
- [ ] Tests cover cross-identity grant isolation.

## Non-Goals

- App store/catalog work.
- Browser origin permissions.
- App-specific UI inside Console.

## Notes

Keep the daemon/app boundary generic. Console may display app names and
capabilities, but protocol code must not hardcode Pastey or Spoke concepts.

