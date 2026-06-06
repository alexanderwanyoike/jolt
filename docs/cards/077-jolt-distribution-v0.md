# 077: Jolt Distribution v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready after 072
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

- [ ] A user can install or unpack Jolt without building from source.
- [ ] Console can start/manage the daemon from the packaged build.
- [ ] CLI is available from the package or documented install path.
- [ ] First-run setup is documented.
- [ ] Pastey and Spoke docs can point to the packaged Jolt requirement.
- [ ] Linux is verified locally.
- [ ] Mac and Windows support limitations are documented if not verified.

## Non-Goals

- OS service/autostart.
- System tray/menu bar presence.
- App store distribution.
- Console Apps page.
- Installing Pastey or Spoke from Console.

## Notes

Keep this boring. The goal is to let people run Jolt, not to solve every
desktop distribution problem.
