# 003: Testing Strategy and Harness

**Type:** AFK  
**Milestone:** Developer experience / Groundwork  
**Status:** Ready  
**Blocked by:** None

## Why

Jolt should not require a Hetzner server plus two physical computers for normal development. Real machines remain valuable for final confidence, but most protocol work must be locally testable.

## What to Build

Define and automate the test layers for Jolt:

1. Pure protocol tests.
2. In-process multi-node tests using TCP transport.
3. Daemon/API end-to-end tests on random local ports.
4. Patchbay topology tests for Linux/manual/CI-special runs.
5. Iroh smoke tests for network-dependent behavior.
6. Real-world canary checklist using Hetzner/laptop/mobile when needed.

The goal is to make it obvious which command to run for each layer and which tests are expected to be deterministic.

## Acceptance Criteria

- [ ] `README.md` has a clear test matrix.
- [ ] A script or documented command runs the normal deterministic test suite.
- [ ] Network-dependent iroh tests are marked ignored/manual or moved behind a separate command.
- [ ] Patchbay tests are documented as Linux/network-namespace tests.
- [ ] Real-world Hetzner/two-device validation is documented as a manual canary, not the normal dev loop.
- [ ] Existing tests that hang in normal `cargo test --workspace` are fixed, ignored, or documented with a clear reason.

## Notes

The preferred local default is:

```text
cargo test -p dweb-core -p dweb-identity -p dweb-store -p dweb-node
cargo test -p dweb-network fetch_manager::
cargo test -p dweb-network bootstrap::
cargo test -p dweb-network protocol::
```

Then expand as the test harness improves.

