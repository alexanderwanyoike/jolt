# 087: Spoke Distribution v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready after 086
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
- document that release signing is configured privately without exposing
  signing key material or private automation details;
- document the simple demo path with Jolt Console approval.

## Acceptance Criteria

- [ ] Tagged Spoke releases publish a Linux AppImage.
- [ ] `curl -fsSL .../install-spoke.sh | bash` installs Spoke.
- [ ] Installer supports `--check`, `--update`, `--force`, and `--dry-run`.
- [ ] Spoke can check for a signed update.
- [ ] Spoke can install and relaunch after a signed update.
- [ ] CI uploads workflow artifacts for PRs and release assets for tags.
- [ ] README explains install, update, and Jolt dependency.
- [ ] Manual smoke: installed Spoke requests access through Console and
      publishes/reads a post.

## Non-Goals

- Bundling Jolt inside Spoke.
- App store distribution.
- Feed/reply product redesign.

## Notes

Use a Spoke-specific updater signing key.
