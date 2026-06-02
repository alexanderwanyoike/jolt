# 032: Built-In Space Lens Demo

**Type:** HITL
**Milestone:** M6 / Product Proof
**Status:** Superseded by external app prototypes
**Blocked by:** 026, 028

## Why

Jolt should demonstrate an application-shaped experience before building a WASM runtime.

The protocol work has proved addressing, signed update logs, content fetch, relay pinning, and offline publisher fetch. That is powerful, but it still looks like infrastructure. The next product proof should show that a signed identity-owned space can be experienced as a useful application surface without hardcoding application concepts into the protocol.

This card exists to avoid jumping straight from "files and CIDs" to "full executable WASM app platform."

## Direction

Build a small built-in lens in the local client/dashboard.

The lens should consume ordinary Jolt protocol data:

```text
identity.jolt/...
  -> signed path records
  -> content CIDs
  -> optional signed space manifest or generated HTML view
  -> verified fetched content
```

It should render a simple space experience that is clearly more useful than a raw file browser.

Possible demo shapes:

- creator/community space with posts, media, links, and pinned availability state
- project/release space with builds, notes, versions, and provenance
- research/archive space with datasets, notes, references, and lineage
- event space with schedule, assets, announcements, and member-only placeholders

## Constraints

- Do not build a WASM runtime.
- Do not add profile, feed, gallery, timeline, game, or lens semantics to the protocol layer.
- Keep the protocol interaction generic: resolve path, verify update log, fetch CID, render content based on app-layer schema or view metadata.
- Prefer a built-in dashboard/client route over a new package/runtime.
- Make the demo relay-backed so Alice can go offline and Bob can still view the space through the relay.

## Open Questions

- What is the smallest demo that would make Jolt feel like a product rather than a storage/debug tool?
- Should the first space view use a tiny JSON manifest, generated HTML, or both?
- Should the built-in lens be owner-focused, visitor-focused, or both?
- Which object types are useful enough for a first demo without becoming protocol concepts?

## Acceptance Criteria

- [ ] A short design note chooses the first demo shape.
- [ ] The demo uses existing `.jolt` resolution and fetch APIs.
- [ ] Alice can publish a minimal space view and pin it to her home relay.
- [ ] Bob can open Alice's `.jolt` address and see the space while Alice is offline.
- [ ] The implementation remains above the protocol layer.
- [ ] Docs explain that this is a built-in lens/proof, not the final WASM runtime.

## Non-Goals

- WASM execution.
- App marketplace.
- Plugin installation.
- General schema registry.
- Full permissions or private object access.
- Rich authoring tools.

## Superseded

Pastey proved the more useful direction: application-shaped experiences should live outside the protocol repo and consume Jolt through the daemon/API.

Do not build a built-in lens in the daemon for now. Keep the protocol repo focused on the daemon/app boundary, sessions, permissions, encryption, and app-facing APIs. External apps such as Pastey and Drops should pressure-test the product shape.
