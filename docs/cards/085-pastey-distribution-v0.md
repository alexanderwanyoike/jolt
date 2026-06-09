# 085: Pastey Distribution v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Done
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
- document that packaged Pastey updates are signed and verified.

## Acceptance Criteria

- [x] Tagged Pastey releases publish a Linux AppImage.
- [x] `curl -fsSL .../install-pastey.sh | bash` installs Pastey.
- [x] Installer supports `--check`, `--update`, `--force`, and `--dry-run`.
- [x] Pastey can check for a signed update.
- [x] Pastey can install and relaunch after a signed update.
- [x] CI uploads workflow artifacts for PRs and release assets for tags.
- [x] README explains install, update, and Jolt dependency.
- [x] Manual smoke: installed Pastey requests access through Console and
      publishes a public paste.

## Non-Goals

- App store distribution.
- Private Pastey feature work.
- Spoke distribution.
- Bundling Jolt inside Pastey.

## Notes

Pastey should not share Jolt Console's updater signing key. Use a Pastey-specific
Tauri updater key so one app key cannot sign another app's updates.

## Implementation

Pastey implementation PR:

- https://github.com/alexanderwanyoike/pastey/pull/5

Pastey release PR:

- https://github.com/alexanderwanyoike/pastey/pull/6

## Verification

Pastey PR #5 verification:

- Red: `node scripts/verify-distribution.mjs` failed before implementation on
  missing `.github/workflows/package-pastey.yml`.
- Green: `node scripts/verify-distribution.mjs`
- Green: `bash -n scripts/install-pastey.sh && bash -n scripts/package-pastey.sh`
- Green: `PASTEY_VERSION=v0.1.0 PASTEY_INSTALL_DIR=/tmp/pastey-install-check/bin PASTEY_STATE_DIR=/tmp/pastey-install-check/state bash scripts/install-pastey.sh --dry-run`
- Green: fake-current-state `--check` reports Pastey up to date.
- Green: `scripts/package-pastey.sh --dry-run`
- Green: `npm test`
- Green: `npm run build`
- Green: `cargo check --manifest-path src-tauri/Cargo.toml --locked`
- Green: unsigned `scripts/package-pastey.sh` builds
  `Pastey_0.1.0_amd64.AppImage`.
- Green: signed local package build produced
  `Pastey_0.1.0_amd64.AppImage.sig`.

Pastey `v0.1.0` release verification:

- Green: release workflow `Package Pastey` completed successfully for tag
  `v0.1.0`.
- Green: GitHub release `v0.1.0` includes:
  - `pastey-x86_64.AppImage`;
  - `pastey-x86_64.AppImage.sha256`;
  - `pastey-x86_64.AppImage.sig`;
  - `latest.json`.
- Green: `latest.json` contains `version: 0.1.0` and a `linux-x86_64`
  signed-update entry pointing at the release AppImage.
- Green: temp install with `scripts/install-pastey.sh` downloads
  `pastey-x86_64.AppImage`; installed AppImage responds to
  `--appimage-help`.
- Green: exact curl install path from Pastey `main` installs `v0.1.0` into a
  temp directory; installed AppImage responds to `--appimage-help`.

Manual smoke:

- Green: human-controlled installed Pastey smoke passed with Jolt Console
  approval and public paste publishing.
