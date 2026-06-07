# 082 - Network Node Module Refactor

Type: AFK
Status: In Progress

## Why

`crates/jolt-network/src/node.rs` has grown into the central place for too many
network concerns. It currently owns node construction, swarm event routing,
daemon command handling, relay mesh state, provider resolution, identity-head
gossip, ingress, encryption helpers, publish/fetch flows, diagnostics, and
tests.

That made sense while proving the network path, but it is now hard to review,
hard to test in focused slices, and risky to modify during the v0 freeze.

## Refactor Direction

Keep `NetworkNode` as the actor/facade that owns the libp2p swarm and serializes
daemon commands plus swarm events. Do not introduce broad traits for hypothetical
network backends.

Extract cohesive internal modules behind small interfaces:

- transport/swarm factory for iroh and TCP construction;
- daemon command handling;
- swarm event routing;
- identity-head hint book;
- identity provider resolution and diagnosis;
- relay mesh exchange/exploration;
- local encryption key and encrypted object helpers;
- recipient ingress queue.

## What To Build

Refactor in small PRs with no protocol behavior changes:

1. Extract duplicated transport/swarm construction behind a factory.
2. Move daemon command handling into an internal command module.
3. Move swarm event routing into an internal event module.
4. Extract self-contained state managers, starting with identity-head hints and
   ingress.

Each PR should keep existing public APIs stable and should run the relevant
`jolt-network` tests.

## Acceptance Criteria

- `NetworkNode` remains the public orchestration type.
- The protocol layer remains application-agnostic.
- Refactor PRs do not change wire formats, API response shapes, or persisted
  store formats.
- Focused `jolt-network` tests pass after each slice.
- `node.rs` becomes a readable orchestrator instead of a multi-concern module.

## Verification Notes

- 2026-06-07: First slice extracted iroh/TCP swarm construction into
  `node::transport` and collapsed duplicated constructor initialization.
- Green: `cargo test -p jolt-network --lib -- --nocapture`
- Green: `cargo fmt --check && git diff --check && ./scripts/test-local.sh`
- 2026-06-07: Second slice moved daemon command handling into
  `node::commands`, keeping the existing command-pattern match and
  `NetworkNode` actor ownership intact.
- Green: `cargo test -p jolt-network --lib -- --nocapture`
- Green: `cargo fmt --check && git diff --check && ./scripts/test-local.sh`
- 2026-06-07: Third slice moved swarm event routing into `node::events`,
  keeping the existing event match and `NetworkNode` actor ownership intact.
- Green: `cargo test -p jolt-network --lib -- --nocapture`
- Green: `cargo fmt --check && git diff --check && ./scripts/test-local.sh`
- 2026-06-07: Fourth slice extracted identity-head hint bookkeeping into
  `node::identity_heads::IdentityHeadHintBook`, keeping node-level gossip,
  diagnosis, and provider candidate behavior intact.
- Green: `cargo test -p jolt-network --lib -- --nocapture`
- Green: `cargo fmt --check && git diff --check && ./scripts/test-local.sh`
