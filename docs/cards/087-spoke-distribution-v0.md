# 087: Spoke Distribution v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Needs Manual Smoke
**Blocked by:** 086

## Why

Once Spoke has a desktop shell, it needs the same boring install/update path as
Pastey. The demo should not ask people to clone the Spoke repo.

## What to Build

In the Spoke repository:

- add GitHub Actions Linux AppImage packaging;
- publish stable release assets:
  - `spoke-x86_64.AppImage`;
  - `spoke-x86_64.AppImage.sha256`;
  - `spoke-x86_64.AppImage.sig`;
  - `latest.json`;
- add a curlable install/update script;
- add Tauri updater plugin/config and a minimal in-app update check/install
  surface;
- document that packaged Spoke updates are signed and verified;
- document the simple demo path with Jolt Console approval.

## Acceptance Criteria

- [x] Tagged Spoke releases publish a Linux AppImage.
- [x] `curl -fsSL .../install-spoke.sh | bash` installs Spoke.
- [x] Installer supports `--check`, `--update`, `--force`, and `--dry-run`.
- [x] Spoke can check for a signed update.
- [x] Spoke can install and relaunch after a signed update.
- [x] CI uploads workflow artifacts for PRs and release assets for tags.
- [x] README explains install, update, and Jolt dependency.
- [ ] Manual smoke: installed Spoke requests access through Console and
      publishes/reads a post.

## Non-Goals

- Bundling Jolt inside Spoke.
- App store distribution.
- Feed/reply product redesign.

## Notes

Use a Spoke-specific updater signing key.

## Implementation

Spoke implementation PR:

- https://github.com/alexanderwanyoike/spoke/pull/6

## Verification

Spoke PR #6 verification:

- Red: `node scripts/verify-distribution.mjs` failed before implementation on
  missing `.github/workflows/package-spoke.yml`.
- Green: `node scripts/verify-distribution.mjs`
- Green: `npm test`
- Green: `npm run build`
- Green: `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- Green: `bash -n scripts/install-spoke.sh && bash -n scripts/package-spoke.sh`
- Green: `scripts/package-spoke.sh --dry-run`
- Green: `SPOKE_VERSION=v0.1.0 SPOKE_INSTALL_DIR=/tmp/spoke-install-check/bin SPOKE_STATE_DIR=/tmp/spoke-install-check/state bash scripts/install-spoke.sh --dry-run`
- Green: fake-current-state `--check` reports Spoke up to date.
- Green: unsigned `scripts/package-spoke.sh` builds
  `Spoke_0.1.0_amd64.AppImage`.
- Green: signed local package build produced
  `Spoke_0.1.0_amd64.AppImage.sig`.
- Green: generated `latest.json` contains the `linux-x86_64`
  signed-update entry.
- Green: generated AppImage responds to `--appimage-help`.

Spoke `v0.1.0` release verification:

- Green: created initial Spoke `main` branch from `dev`.
- Green: tagged Spoke `v0.1.0` from `main`.
- Green: GitHub `Package Spoke` tag workflow published release assets:
  - `spoke-x86_64.AppImage`;
  - `spoke-x86_64.AppImage.sha256`;
  - `spoke-x86_64.AppImage.sig`;
  - `latest.json`.
- Green: temp curl install from `main` installed `v0.1.0` into
  `/tmp/spoke-v010-install-check/bin/spoke`.
- Green: installed AppImage responds to `--appimage-help`.

Remaining manual smoke:

- Run installed Spoke with Jolt Console, approve the app session, and publish a
  post through the installed AppImage.
