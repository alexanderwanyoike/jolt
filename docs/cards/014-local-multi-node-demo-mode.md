# 014: Local Multi-Node Demo Mode

**Type:** AFK
**Milestone:** Developer experience / M3.5
**Status:** Ready
**Blocked by:** 002

## Why

The local dashboard makes a single daemon visible, but two local daemons do not currently discover or connect to each other in a predictable way.

This is confusing because the project claims local discovery, and the TCP-based tests do prove local peer transfer. The real daemon path uses iroh transport, and its current listen address is only `/p2p/<peer-id>`, which is not a useful LAN dial address for mDNS-based local demos.

Developers should be able to run two local nodes, see them connect in the dashboards, publish content on one, and fetch it from the other without needing Hetzner, Docker, or two physical machines.

## What to Build

Add a deterministic local multi-node demo path.

Pick the smallest implementation that makes the flow reliable. Good candidates:

- A daemon flag such as `--transport tcp` or `--dev-tcp` that uses `NetworkNode::new_tcp`.
- A manual peer connect API/dashboard control that dials a supplied multiaddr.
- A better iroh local pairing path if iroh exposes a stable local node address we can display and dial.

The likely first version is a dev TCP mode, because the existing tests already prove that transport locally.

The demo should support:

- Start node A on one API port and one P2P port.
- Start node B on another API port and another P2P port.
- Connect automatically through mDNS or explicitly through a documented dial/connect command.
- Show connected peers in both dashboards.
- Publish text/file on node A.
- Fetch the CID from node B.
- Show cache/published counts update after the transfer.

## Acceptance Criteria

- [ ] Docs include a copy-paste local two-node demo using only one machine.
- [ ] Node A and node B use separate data directories and API ports.
- [ ] The dashboards show each other as connected peers.
- [ ] Node A can publish content through the dashboard.
- [ ] Node B can fetch that CID through the dashboard.
- [ ] Node B caches the fetched content and the dashboard reflects it.
- [ ] The implementation does not change the default real-network iroh behavior.
- [ ] The test suite includes a daemon/API level check for the local two-node path.

## Notes

This is developer/demo infrastructure, not the final networking story.

Do not hide the iroh issue. Document the distinction clearly:

- Runtime/default transport: iroh for real P2P and NAT traversal.
- Deterministic local demo transport: TCP, unless iroh local pairing is made equally reliable.

This card can be implemented before the broader testing strategy card because it gives the dashboard a meaningful multi-node workflow to observe.
