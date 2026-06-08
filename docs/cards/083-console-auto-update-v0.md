# 083: Jolt Console Auto Update v0

**Type:** AFK after design  
**Milestone:** Post-v0 Distribution  
**Status:** Ready after 077  
**Blocked by:** 077

## Why

The v0 Linux AppImage can be installed quickly, but updating still requires the
user to rerun the curl installer. That is acceptable for the first release, but
it leaves the product loop open: users should be able to learn that a new Jolt
Console exists, install it safely, and restart without reading release notes or
remembering a shell command.

Because the Console package includes the daemon sidecar, update behavior must be
deliberate. Updating the shell while an old Console-owned daemon is running must
not leave the local runtime in a confusing state.

## Direction

Use Tauri's signed updater for the packaged Console. Keep
`scripts/install-jolt-console.sh` as the fallback/manual repair path.

The update flow should be user-approved, not silent:

1. Console checks for updates on startup and through a manual Settings action.
2. If a newer signed release exists, Console shows the version and release notes.
3. The user chooses to install the update.
4. Console downloads and verifies the signed update artifact.
5. Console stops the daemon only if it owns the daemon process.
6. Console installs the update and relaunches.
7. On restart, Console starts the bundled daemon sidecar normally.

## What to Build

- Add Tauri updater support to `apps/jolt-console`.
- Add the Tauri process plugin if needed for relaunch.
- Generate and document the updater signing key process.
- Store the updater public key in the Tauri config.
- Require the private signing key through GitHub Actions secrets for tagged
  release builds.
- Generate updater artifacts and signatures during packaging.
- Publish a release update manifest such as `latest.json`.
- Add Console UI for:
  - update status;
  - manual check;
  - update available;
  - install progress;
  - install/relaunch failure.
- Preserve the curl installer path as the documented fallback.

## Acceptance Criteria

- [ ] CI creates signed updater artifacts for tagged Console releases.
- [ ] Release assets include the updater signature and update manifest required
      by the packaged Console.
- [ ] Console can check for an update from a packaged build.
- [ ] Console can install a newer signed update and relaunch.
- [ ] Console does not stop an externally managed daemon during update.
- [ ] Console stops a Console-owned daemon before relaunch if required.
- [ ] If update check/install fails, the error is visible and the existing
      Console remains usable.
- [ ] `scripts/install-jolt-console.sh` remains documented as a fallback.
- [ ] The update path is tested against a local or fake manifest before relying
      on GitHub releases.

## Non-Goals

- Silent background updates.
- OS service installation.
- System tray/menu-bar update notifications.
- App catalog/store distribution.
- macOS and Windows updater verification unless those packages are built in the
  same slice.

## Security Notes

Do not implement this as "download and run a shell script from the app".

The update path must verify signed update artifacts. HTTPS and GitHub release
ownership are useful transport properties, but they are not enough for an
in-app updater that replaces the local runtime.

The updater private key must never be committed. It should live in release
automation secrets. The public key can be committed in the Tauri configuration.

## Testing Notes

Use TDD where practical around the Console update state machine and daemon
lifecycle behavior. The native updater itself should be verified with an
end-to-end packaged update test using a local/static manifest before the GitHub
release path is trusted.

