# 054: Pastey Two-Node Local Demo Harness

**Type:** AFK  
**Milestone:** Developer Experience / App Dogfooding  
**Status:** Ready  
**Blocked by:** None

## Why

Pastey proved that a separate app can use Jolt locally, but the demo setup is manual: start Alice daemon, start Bob daemon, connect peers, start two Pastey clients with different daemon URLs, then copy a `.jolt` address.

Developers should have a repeatable one-machine demo harness for this flow.

## What to Build

Add a script or documented harness that starts:

- Alice daemon with isolated data dir.
- Bob daemon with isolated data dir.
- Local TCP transport and fixed P2P/API ports.
- Peer connection between Alice and Bob.
- Pastey client for Alice.
- Pastey client for Bob.

The harness should print:

- Alice Pastey URL.
- Bob Pastey URL.
- Alice identity address.
- Any published sample paste address if the harness creates one.

## Acceptance Criteria

- [ ] One command starts Alice and Bob daemons locally.
- [ ] One command starts or clearly instructs how to start two Pastey clients.
- [ ] Bob is connected to Alice.
- [ ] Alice can publish a paste.
- [ ] Bob can fetch Alice's paste through the Bob Pastey client.
- [ ] Cleanup stops all spawned processes.
- [ ] The harness does not require Docker, Hetzner, or multiple machines.

## Notes

This supersedes the old local multi-node dashboard demo direction. The app surface is now the more useful demo.
