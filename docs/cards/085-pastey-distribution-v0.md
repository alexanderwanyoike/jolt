# 085: Pastey Distribution v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready
**Blocked by:** 079

## Why

Pastey is a useful technical proof that an external app can use Jolt through
scoped app sessions. For the demo to be simple, users should not build Pastey
from source or run Vite manually.

Pastey should be installable as a desktop app from its own release.

## What to Build

In the Pastey repository:

- add GitHub Actions Linux AppImage packaging;
- publish stable release assets:
  - `pastey-x86_64.AppImage`;
  - `pastey-x86_64.AppImage.sha256`;
  - `pastey-x86_64.AppImage.sig`;
  - `latest.json`;
- add a curlable install/update script;
- add Tauri updater plugin/config and a minimal in-app update check/install
  surface;
- document that Pastey requires a running Jolt daemon/Console;
- document required repository secrets for signed updates.

## Acceptance Criteria

- [ ] Tagged Pastey releases publish a Linux AppImage.
- [ ] `curl -fsSL .../install-pastey.sh | bash` installs Pastey.
- [ ] Installer supports `--check`, `--update`, `--force`, and `--dry-run`.
- [ ] Pastey can check for a signed update.
- [ ] Pastey can install and relaunch after a signed update.
- [ ] CI uploads workflow artifacts for PRs and release assets for tags.
- [ ] README explains install, update, and Jolt dependency.
- [ ] Manual smoke: installed Pastey requests access through Console and
      publishes a public paste.

## Non-Goals

- App store distribution.
- Private Pastey feature work.
- Spoke distribution.
- Bundling Jolt inside Pastey.

## Notes

Pastey should not share Jolt Console's updater signing key. Use a Pastey-specific
Tauri updater key so one app key cannot sign another app's updates.
