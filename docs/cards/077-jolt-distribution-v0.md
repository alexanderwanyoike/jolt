# 077: Jolt Distribution v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Extended in PR
**Blocked by:** 064, 072

## Why

Jolt cannot be judged as a product if users must understand the repo to run it.
v0 needs a realistic installation/run story for the local runtime.

## What to Build

Make Jolt distributable as:

```text
Jolt Console + daemon + CLI
```

The distribution should support:

- Linux first, with Mac/Windows constraints documented;
- first-run identity creation;
- daemon startup from Console;
- CLI available for diagnostics;
- clear uninstall/reset instructions;
- clear app integration instructions for Pastey and Spoke.

## Acceptance Criteria

- [x] A user can install or unpack Jolt without building from source.
- [x] Console can start/manage the daemon from the packaged build.
- [x] CLI is available from the package or documented install path.
- [x] First-run setup is documented.
- [x] Pastey and Spoke docs can point to the packaged Jolt requirement.
- [x] Linux is verified locally.
- [x] Mac and Windows support limitations are documented if not verified.
- [x] Cross-platform CI packaging publishes stable macOS and Windows artifact
      names.

## Non-Goals

- OS service/autostart.
- System tray/menu bar presence.
- App store distribution.
- Console Apps page.
- Installing Pastey or Spoke from Console.

## Notes

Keep this boring. The goal is to let people run Jolt, not to solve every
desktop distribution problem.

## Implementation Notes

Added a Linux-first packaging path for:

```text
Jolt Console + bundled jolt daemon/CLI sidecar
```

The package script builds the release `jolt` binary, stages it as the Tauri
sidecar, builds the Console web assets, and produces an AppImage under
`target/release/bundle/appimage/`.

The AppImage was built locally on Linux and inspected to confirm it includes
both `jolt-console` and the bundled `jolt` sidecar. macOS and Windows remain
documented but unverified for v0.

GitHub Actions now builds the same AppImage in CI, uploads
`jolt-console-x86_64.AppImage` as a workflow artifact, and publishes that stable
asset name on tagged releases. `scripts/install-jolt-console.sh` provides the
curlable install/update path for Linux users.

Human GUI verification of daemon startup from the packaged AppImage is still a
good final check before calling the release artifact user-ready.

Cross-platform extension:

- `scripts/package-jolt-console.sh` now accepts `--bundle appimage`, `--bundle
  dmg`, or `--bundle nsis`, defaulting by host OS.
- GitHub Actions builds native packages on Linux, macOS, and Windows runners.
- Tagged releases normalize these stable assets:

```text
jolt-console-x86_64.AppImage
jolt-console-aarch64.dmg
jolt-console-aarch64.app.tar.gz
jolt-console-x86_64-setup.exe
jolt-linux-x86_64
jolt-macos-aarch64
jolt-windows-x86_64.exe
latest.json
```

The macOS `.app.tar.gz` asset is the signed updater payload; users install from
the `.dmg`. macOS and Windows packages still need human install/update smoke
tests and production signing/notarization before they should be described as
user-ready.
