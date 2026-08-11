#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> Running deterministic local Jolt test suite"
echo "==> This excludes ignored manual tests for iroh smoke checks and patchbay topologies"
echo "==> This excludes the Tauri desktop shell, which has native Linux WebKit/GTK prerequisites"

cargo test --locked --workspace --exclude jolt-console
cargo build --locked -p jolt-node
./scripts/test-relay-pin-allowlist-process.sh
./scripts/test-pastey-two-node-demo-harness.sh
./scripts/test-install-jolt-console.sh
