# 001: Stabilize Current State and Test Harness

**Type:** AFK  
**Milestone:** Groundwork before M4  
**Status:** Ready  
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
- Is Docker still supported or archived?
- Why do iroh-backed unit tests hang locally, and should they use TCP transport instead?

## Acceptance Criteria

- [ ] `cargo test` guidance in `README.md` matches the tests that actually work.
- [ ] Stale Docker commands using old CLI flags are fixed or the Docker harness is explicitly archived.
- [ ] iroh-backed unit tests either pass reliably or are moved behind an ignored/manual test path.
- [ ] `cargo test -p jolt-core -p jolt-identity -p jolt-store -p jolt-node` passes.
- [ ] Focused `jolt-network` unit tests pass without hanging.
- [ ] Known manual/real-hardware tests are documented separately from regular CI/local tests.

## Notes

The previous local check showed the non-network crates passing and focused `jolt-network` tests passing. Full workspace tests hung in `NetworkNode::new` tests using iroh transport.

