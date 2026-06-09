# 086: Spoke Desktop Shell v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** Ready
**Blocked by:** 078

## Why

Spoke is the human-facing social PoC, but it is currently a Vite web app. The
v0 demo should be desktop-app first: users install Jolt, install Spoke, approve
Spoke in Console, then post/reply without running dev servers.

## What to Build

In the Spoke repository:

- add a Tauri desktop shell around the existing Vite app;
- add desktop scripts:
  - `desktop:dev`;
  - `desktop:build`;
  - `tauri`;
- add a Tauri daemon bridge for Jolt HTTP calls where direct browser/proxy
  assumptions do not work in packaged mode;
- preserve the current app-session flow and local daemon URL defaults;
- set Linux AppImage as the first bundle target;
- add basic Tauri-side tests where practical.

## Acceptance Criteria

- [ ] `npm run desktop:dev` starts Spoke in a Tauri window.
- [ ] `npm run desktop:build` produces a Linux AppImage locally.
- [ ] Packaged Spoke can request app access from Jolt Console.
- [ ] Packaged Spoke can publish a profile/post.
- [ ] Packaged Spoke can read known-contact posts.
- [ ] Packaged Spoke can send and accept a reply using existing Jolt APIs.
- [ ] README documents desktop dev/build usage.

## Non-Goals

- GitHub release packaging.
- Auto-update.
- Redesigning Spoke's social model.
- App store distribution.

## Notes

Keep protocol boundaries clean. Spoke-specific concepts such as profiles,
posts, feeds, contacts, and replies stay in the Spoke repo as signed app
content, not in Jolt protocol code.
