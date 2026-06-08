# 001: Stabilize Current State and Test Harness

**Type:** AFK  
**Milestone:** Groundwork before M4  
**Status:** Done
**Blocked by:** None

## Why

Jolt already proved P2P content transfer across real machines, NAT, and CGNAT. The repo still has drift from earlier approaches: Docker scripts are stale, some docs say old things, and `cargo test --workspace` can hang in iroh-backed node tests.

Before adding mutable records or relays, make the current proof easy to understand and verify.

## What to Build

Create a reliable local verification path for the existing implementation and clearly mark which network tests are current, stale, or manual.

This should answer:

- Which tests should a developer run locally?
- Which tests require patchbay/network namespaces?
- Which tests require real internet/iroh behavior?
- Is Docker still supported?
- Why do iroh-backed unit tests hang locally, and should they use TCP transport instead?

## Acceptance Criteria

- [x] `cargo test` guidance in `README.md` matches the tests that actually work.
- [x] Stale Docker commands using old CLI flags are removed with the unsupported Docker harness.
- [x] iroh-backed unit tests either pass reliably or are moved behind an ignored/manual test path.
- [x] `cargo test -p jolt-core -p jolt-identity -p jolt-store -p jolt-node` passes.
- [x] Focused `jolt-network` unit tests pass without hanging.
- [x] Known manual/real-hardware tests are documented separately from regular CI/local tests.

## Result

The supported local verification path is `./scripts/test-local.sh`, which runs `cargo test --locked --workspace`.

Manual/network-dependent paths are documented separately:

- iroh smoke test: `cargo test -p jolt-network new_iroh_creates_node_without_error -- --ignored`
- patchbay topology tests: `cargo test -p jolt-network --test nat_traversal -- --ignored`
- real-world canaries: documented in `docs/13-three-node-canary.md` and `docs/14-relay-mesh-milestone-canary.md`

The old Docker topology harness has been removed. It no longer matches the supported verification strategy and was more likely to confuse future work than provide useful confidence.

Verification completed:

- `./scripts/test-local.sh`
- `cargo test -p jolt-core -p jolt-identity -p jolt-store -p jolt-node`
- `cargo test -p jolt-network --lib`
- `cargo test -p jolt-network --tests`

## Notes

The iroh endpoint smoke test remains available as an ignored manual test because it can depend on local network and relay availability. The deterministic workspace path no longer runs that smoke test by default.
