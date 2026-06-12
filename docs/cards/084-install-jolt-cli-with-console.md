# 084: Install Jolt CLI with Console

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Extended in PR
**Blocked by:** 077, 083

## Why

Jolt Console already bundles the `jolt` runtime binary as a Tauri sidecar, but
installing the Console AppImage does not make `jolt` callable from the user's
shell. That weakens the one-product story and makes relay setup/debugging feel
like a source checkout problem.

The installed Jolt experience should provide both:

```text
~/.local/bin/jolt-console
~/.local/bin/jolt
```

Console remains the desktop control surface. `jolt` remains the runtime,
CLI, daemon, and relay operator binary.

## What to Build

- Publish standalone Linux, macOS, and Windows `jolt` binary assets from the
  same tagged release as Jolt Console.
- Extend the install script so the normal curl install installs or updates both
  `jolt-console` and `jolt`.
- Make CLI install asset selection platform-aware:
  - Linux x86_64: `jolt-linux-x86_64` as `jolt`.
  - macOS Apple Silicon: `jolt-macos-aarch64` as `jolt`.
  - Windows x86_64 under Git Bash/MSYS: `jolt-windows-x86_64.exe` as
    `jolt.exe`.
- Keep the Console AppImage sidecar behavior unchanged; Console can still use
  its bundled sidecar.
- Record installed versions for both assets.
- Support `--check`, `--update`, `--force`, and `--dry-run` for the combined
  install path.
- Document that relay/server users can install only the `jolt` binary if they
  do not need the desktop Console.

Concrete v0 release assets:

```text
jolt-console-x86_64.AppImage
jolt-linux-x86_64
jolt-macos-aarch64
jolt-windows-x86_64.exe
```

The Linux default install path creates both `jolt-console` and `jolt`. macOS and
Windows default to CLI-only because the DMG and setup EXE are user-facing
Console installers, not direct shell commands. Headless relay/server installs
can use:

```bash
scripts/install-jolt-console.sh --cli-only
```

## Acceptance Criteria

- [x] Tagged releases publish `jolt-console-x86_64.AppImage`.
- [x] Tagged releases publish standalone Linux, macOS, and Windows `jolt`
      binaries.
- [x] Curl install creates executable `jolt-console` and `jolt` commands on
      Linux.
- [x] Curl install creates executable `jolt` on macOS.
- [x] Curl install creates executable `jolt.exe` on Windows under Git
      Bash/MSYS.
- [x] `jolt --version` or equivalent reports the tagged version.
- [x] Existing Console auto-update behavior still works.
- [x] Installer can check whether either installed asset is stale.
- [x] README install docs show both desktop and headless/server paths.
- [x] CI verifies release asset names and installer markers.

## Non-Goals

- Native macOS/Windows Console installer automation from the Bash installer.
- OS service installation.
- Relay configuration UX.
- Installing Pastey or Spoke.

## Notes

This card repairs the last gap in card 077: the package includes `jolt`, but the
installer does not expose it as a user-callable CLI yet.

## Verification

- Red: `node scripts/verify-distribution.mjs` failed on missing
  `jolt-linux-x86_64` workflow marker before implementation.
- Green: `node scripts/verify-distribution.mjs`
- Green: `bash -n scripts/install-jolt-console.sh`
- Green: `JOLT_VERSION=v0.3.2 JOLT_INSTALL_DIR=/tmp/jolt-install-check/bin JOLT_STATE_DIR=/tmp/jolt-install-check/state bash scripts/install-jolt-console.sh --dry-run`
- Green: `JOLT_VERSION=v0.3.2 JOLT_INSTALL_DIR=/tmp/jolt-install-check/bin JOLT_STATE_DIR=/tmp/jolt-install-check/state bash scripts/install-jolt-console.sh --cli-only --dry-run`
- Green: `./scripts/test-install-jolt-console.sh`
- Green: fake-current-state `--check` reports both Jolt Console and Jolt CLI
  up to date when both version files and executables exist.
- Green: `cargo run --locked -p jolt-node -- --version`
- Green: `./scripts/test-local.sh`

Release-time note: the new standalone CLI asset is produced by the tag workflow;
the current `v0.3.2` release predates this card and does not contain
`jolt-linux-x86_64`.
