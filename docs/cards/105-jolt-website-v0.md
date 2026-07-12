# 105: Jolt Website v0

**Type:** AFK after strategy
**Milestone:** Project Communication
**Status:** Ready after 104
**Blocked by:** 104

## Why

After the website strategy is agreed, Jolt needs a simple public site that can
serve as the canonical project entry point.

The site should help a new person answer:

- what is Jolt;
- why identities and communities matter;
- what can I try today;
- what is still experimental;
- where are the RFCs and implementation docs.

## What to Build

Create the first website:

- homepage with the plain-language project thesis;
- current status and limitations page;
- install/demo page for Jolt Console, Spoke, and Pastey where relevant;
- concepts pages for identity, devices, apps, communities, encrypted content,
  relays, and availability;
- RFC index page;
- links back to source docs and cards;
- deployment path, such as GitHub Pages or another static host.

## Acceptance Criteria

- [ ] The website can be built locally with one documented command.
- [ ] The website has a clear homepage and navigation.
- [ ] The website links to install/demo instructions.
- [ ] The website links to the RFC index.
- [ ] The website states current limitations honestly.
- [ ] The website does not require a running Jolt node.
- [ ] CI verifies the site build or static output.

## Non-Goals

- User accounts.
- Hosted app catalog.
- Dynamic community search.
- Replacing in-repo implementation docs.

## Notes

If this is built in the repo, prefer a simple static stack and keep the site
content easy to review in PRs.
