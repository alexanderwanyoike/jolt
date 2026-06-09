# 084: Install Jolt CLI with Console

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready
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

- Publish a standalone Linux `jolt` binary asset from the same tagged release
  as Jolt Console.
- Extend the install script so the normal curl install installs or updates both
  `jolt-console` and `jolt`.
- Keep the Console AppImage sidecar behavior unchanged; Console can still use
  its bundled sidecar.
- Record installed versions for both assets.
- Support `--check`, `--update`, `--force`, and `--dry-run` for the combined
  install path.
- Document that relay/server users can install only the `jolt` binary if they
  do not need the desktop Console.

## Acceptance Criteria

- [ ] Tagged releases publish `jolt-console-x86_64.AppImage`.
- [ ] Tagged releases publish a standalone Linux `jolt` binary or tarball.
- [ ] Curl install creates executable `jolt-console` and `jolt` commands.
- [ ] `jolt --version` or equivalent reports the tagged version.
- [ ] Existing Console auto-update behavior still works.
- [ ] Installer can check whether either installed asset is stale.
- [ ] README install docs show both desktop and headless/server paths.
- [ ] CI verifies release asset names and installer markers.

## Non-Goals

- macOS/Windows packaging.
- OS service installation.
- Relay configuration UX.
- Installing Pastey or Spoke.

## Notes

This card repairs the last gap in card 077: the package includes `jolt`, but the
installer does not expose it as a user-callable CLI yet.
