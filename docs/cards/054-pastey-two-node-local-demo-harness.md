# 054: Pastey Two-Node Local Demo Harness

**Type:** AFK  
**Milestone:** Developer Experience / App Dogfooding  
**Status:** Implemented in PR  
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

- [x] One command starts Alice and Bob daemons locally.
- [x] One command starts or clearly instructs how to start two Pastey clients.
- [x] Bob is connected to Alice.
- [x] Alice can publish a paste.
- [x] Bob can fetch Alice's paste through the Bob Pastey client.
- [x] Cleanup stops all spawned processes.
- [x] The harness does not require Docker, Hetzner, or multiple machines.

## Implementation Notes

- Added `scripts/pastey-two-node-demo.sh`.
- Default mode starts:
  - Alice daemon on API `9871`, P2P `4901`.
  - Bob daemon on API `9872`, P2P `4902`.
  - Alice Pastey on `5174`.
  - Bob Pastey on `5175`.
- `--smoke --no-pastey` runs a non-interactive app API proof:
  - create and approve scoped app sessions;
  - publish Alice's `/pastes/two-node-demo`;
  - fetch Alice's `.jolt` paste through Bob's app session.
- `--dry-run` prints the planned ports, URLs, data dirs, and smoke behavior for
  a fast focused test.

## Verification

- Red: `./scripts/test-pastey-two-node-demo-harness.sh` failed while the
  harness script was missing.
- Green:
  - `./scripts/test-pastey-two-node-demo-harness.sh`
  - `bash -n scripts/pastey-two-node-demo.sh scripts/test-pastey-two-node-demo-harness.sh`
  - `./scripts/pastey-two-node-demo.sh --smoke --no-pastey`
  - timeout-based interactive startup smoke verified both Pastey dev clients
    start and cleanup stops spawned processes.

## Notes

This supersedes the old local multi-node dashboard demo direction. The app surface is now the more useful demo.
