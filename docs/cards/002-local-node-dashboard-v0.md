# 002: Local Node Dashboard v0

**Type:** AFK  
**Milestone:** Developer experience / M3.5  
**Status:** Ready  
**Blocked by:** None

## Why

Working only through CLI and curl makes Jolt hard to understand and boring to demo. Nodes and relays need a simple localhost dashboard that shows what the daemon thinks is happening.

This is not the future app platform UI. It is a debugging console for development and demos.

## What to Build

Serve a local dashboard from the existing HTTP daemon.

The first version should show:

- Local peer ID.
- Uptime.
- Listen addresses.
- Connected peers.
- Direct vs relayed peer counts.
- Published content count.
- Cached content count.
- Cache stats.
- Recent cache entries.
- Publish file/text form.
- Fetch by CID form.
- Basic request result/error display.

If relay/home-relay data is not implemented yet, include a placeholder section that says it is not configured/available.

## Acceptance Criteria

- [ ] Visiting the daemon root or a clear path such as `/dashboard` shows a browser UI.
- [ ] Dashboard fetches node status from existing API endpoints.
- [ ] Dashboard lists peers and cache entries using existing API endpoints.
- [ ] Dashboard can publish content through the existing publish endpoint.
- [ ] Dashboard can fetch content by CID through the existing fetch endpoint.
- [ ] UI works with a locally running daemon and requires no build toolchain.
- [ ] Dashboard is localhost-first and does not expose new remote attack surface by default.

## Notes

Keep this deliberately simple:

- Static HTML/CSS/JS served by `dweb-server`.
- No React/Vite build unless there is a strong reason.
- No auth system yet; preserve localhost-only default.
- Design should feel like a quiet node console, not a marketing page.

