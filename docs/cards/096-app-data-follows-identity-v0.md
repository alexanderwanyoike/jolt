# 096: App Data Follows Identity v0

**Type:** AFK after design  
**Milestone:** Identity and Device Sprint  
**Status:** Ready after 094 and 095  
**Blocked by:** 094, 095

## Why

For apps like Pastey, users expect "my pastes follow me." That should not mean
Jolt automatically syncs every content byte for every app. It means the app's
identity-scoped index follows the user, and content can be fetched, cached, or
pinned according to policy. For private app data, the index itself may also be
encrypted so a new device can see "my pastes" only after it has the right
device encryption grant.

The product promise should be:

```text
open Pastey on a new authorized device;
approve Pastey for this identity;
see the paste list;
fetch paste bodies on demand;
optionally keep the app data on this device for offline use.
```

## What to Build

Add or formalize the generic primitives apps need for identity-scoped app data:

- publish an app-owned index/reference under a user identity;
- support encrypted app-owned indexes for private app data;
- resolve the current merged app index across authorized devices;
- let apps fetch referenced CIDs lazily;
- let apps fetch encrypted referenced CIDs and decrypt them through the daemon
  when the device is authorized;
- let apps request local keep/pin behavior for their own referenced content;
- make app indexes work with multi-writer identity resolution;
- prove the path with Pastey or a Pastey-like test app.

## Acceptance Criteria

- [ ] An app can publish an identity-scoped index without Jolt understanding the
      app's schema.
- [ ] An app can publish an encrypted identity-scoped index for private app
      data without Jolt understanding the app's schema.
- [ ] A second authorized device can resolve the same app index for that user
      identity.
- [ ] Referenced public content can be fetched lazily from available providers.
- [ ] Referenced encrypted content can be fetched lazily and decrypted only by
      authorized devices.
- [ ] App content can be marked to keep on this device for offline use.
- [ ] The daemon clearly distinguishes app index sync from content byte sync.
- [ ] A Pastey follow-me smoke path is documented or automated.

## Non-Goals

- Automatic full-data sync for every app.
- Protocol-level paste/post/feed semantics.
- Storage quotas or billing.
- Conflict-free editing of the same paste body from two devices.

## Notes

This card should preserve the split:

```text
control plane = signed identity/app references
content plane = CID fetch/cache/pin
private plane = encrypted content plus key grants
```

Encrypted app data may still expose outer metadata such as the existence of an
identity path, CID availability, object size, and timing. Preventing that
metadata leakage is not required for this v0 slice.
