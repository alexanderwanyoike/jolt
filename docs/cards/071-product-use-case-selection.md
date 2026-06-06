# 071: Product Use-Case Selection

**Type:** HITL  
**Milestone:** Product Direction  
**Status:** Discussion next
**Blocked by:** 057, 065, 070

## Why

Jolt has now proved enough infrastructure to stop treating more infrastructure
as the next obvious move. The network can publish, resolve, fetch, pin, approve
scoped apps, encrypt private content, run Pastey externally, manage the daemon
from Console, and diagnose basic relay reachability.

The next risk is product clarity:

> Why would someone run Jolt or join a Jolt-backed community before the network
> is large?

The next card should pick a concrete product/use-case proof that answers that
question better than another protocol or operator slice would.

## What to Decide

Choose one next proof with a sharp user story, for example:

- a stronger Pastey-shaped workflow that centralized paste/share tools make
  awkward;
- a small community/private sharing product loop;
- a local-first collaboration or publishing workflow where identity-owned paths
  matter;
- an app-boundary proof that makes Jolt Console feel like useful local
  infrastructure rather than a demo control panel.

The chosen proof should clarify:

- who the user is;
- what painful workflow they can complete;
- why Jolt's identity-owned publishing, scoped app authority, private sharing,
  or offline relay availability matters;
- what can be shown in a human demo;
- what not to build yet.

## Acceptance Criteria

- [ ] Pick one product/use-case proof to build next.
- [ ] Explain the target user and concrete workflow.
- [ ] Explain why Jolt is materially better suited to this workflow than a
      normal centralized app.
- [ ] Define the smallest demo that proves the workflow end to end.
- [ ] Split the chosen proof into one or more implementation cards.
- [ ] Keep protocol-layer work app-agnostic.

## Non-Goals

- WASM app runtime work.
- Storage markets, payments, or storage-market mechanics.
- Drops.
- Protocol-level inbox, message, contact, feed, profile, or app semantics.
- Relay structured logs and metrics. Those remain useful but are deliberately
  parked behind product/use-case work.

## Notes

Pastey has already been useful as pressure on app sessions, private sharing,
and daemon APIs. The next proof may still be Pastey if it can become a sharper
product story rather than only a test app.

If messaging or bidirectional communication comes back into scope, use the
direction in [058](058-bidirectional-communication-and-signed-reachability-design.md):
Jolt can provide signed reachability metadata and generic identity-authenticated
transport primitives, but application concepts must remain above the protocol.

## Verification

Docs-only planning card. No code tests were run.
