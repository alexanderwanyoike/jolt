# 101: Default Discoverable Communities v0

**Type:** AFK after design
**Milestone:** Community Discovery Sprint
**Status:** Ready after 099
**Blocked by:** 098, 099

## Why

New users need somewhere to start. Jolt can ship default discoverable
communities, but it should not silently join users to anything.

Defaults are a discovery convenience, not authority.

## What to Build

Add a small default community discovery path:

- ship a configurable list of default community identities;
- show default communities as discoverable/watchable;
- let users remove or hide defaults locally;
- let users add a community identity manually;
- support joining defaults through normal community policy;
- expose followed/watched communities to apps.

## Acceptance Criteria

- [ ] A fresh install can show at least one default discoverable community.
- [ ] The user is not automatically joined to default communities.
- [ ] The user can watch, join, hide, or remove a default community locally.
- [ ] The user can add a non-default community identity.
- [ ] Apps can list watched/joined communities through generic APIs.
- [ ] Tests cover default listing, local hide/remove, manual add, and app
      visibility.

## Non-Goals

- Global community search.
- Centralized recommendation service.
- App-specific default feeds.
- Mandatory defaults.

## Notes

The default list should be easy to replace for development/demo builds without
making it a protocol dependency.
