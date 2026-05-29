# 011: WASM Runtime Parking Lot

**Type:** HITL  
**Milestone:** M7+  
**Status:** Later  
**Blocked by:** 010

## Why

The docs describe a full WASM app platform, but the current protocol work should first land mutable identity state and relay availability. WASM can wait until Jolt has a stable content/addressing/relay story and at least one useful non-WASM application proof.

## What to Decide

Before starting runtime work, decide whether Jolt is still aiming for:

- Full local WASM app runtime.
- Browser-served app bundles only.
- Protocol-first publishing/feed system before apps.
- Some smaller app model.
- Built-in lenses over signed spaces before executable lenses.

## Acceptance Criteria

- [ ] A short ADR or design note states when WASM work should restart.
- [ ] The note identifies the smallest useful app demo.
- [ ] The note lists host APIs required by that demo.
- [ ] The note explicitly depends on mutable records and relay availability being usable.
- [ ] The note explains why a built-in non-WASM app demo is insufficient, if WASM is restarted before that demo exists.

## Notes

Do not start this until a relay-backed space/application demo exists. The first demo should probably be a built-in lens in the dashboard/client rather than an executable WASM runtime.
