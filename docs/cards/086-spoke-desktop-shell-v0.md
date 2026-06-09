# 086: Spoke Desktop Shell v0

**Type:** AFK  
**Milestone:** v0 Endgame  
**Status:** In PR
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

- [x] `npm run desktop:dev` starts Spoke in a Tauri window.
- [x] `npm run desktop:build` produces a Linux AppImage locally.
- [x] Packaged Spoke can request app access from Jolt Console.
- [x] Packaged Spoke can publish a profile/post.
- [ ] Packaged Spoke can read known-contact posts.
- [ ] Packaged Spoke can send and accept a reply using existing Jolt APIs.
- [x] README documents desktop dev/build usage.

## Non-Goals

- GitHub release packaging.
- Auto-update.
- Redesigning Spoke's social model.
- App store distribution.

## Notes

Keep protocol boundaries clean. Spoke-specific concepts such as profiles,
posts, feeds, contacts, and replies stay in the Spoke repo as signed app
content, not in Jolt protocol code.

## Implementation

Spoke implementation PR:

- https://github.com/alexanderwanyoike/spoke/pull/5

## Verification

Spoke PR #5 verification:

- Red: `npm test -- src/api.test.ts` failed while desktop runtime still used
  browser `fetch`.
- Green: `npm test -- src/api.test.ts`
- Green: `npm test`
- Green: `npm run build`
- Green: `cargo test --manifest-path src-tauri/Cargo.toml --locked`
- Green: `npm run desktop:build` produced
  `src-tauri/target/release/bundle/appimage/Spoke_0.1.0_amd64.AppImage`.
- Green: generated AppImage responds to `--appimage-help`.
- Smoke: `timeout 20s npm run desktop:dev` started Vite on
  `127.0.0.1:5178` and launched the Tauri binary; the timeout then
  terminated the dev process.

Manual smoke:

- Green: human-controlled packaged Spoke smoke passed for Jolt Console access
  request/approval and profile/post publishing.
- Not covered in this smoke: known-contact feed reading and reply send/accept
  require a second identity.
