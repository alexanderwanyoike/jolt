#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> Running deterministic local Jolt test suite"
echo "==> This excludes ignored manual tests for iroh smoke checks and patchbay topologies"

cargo test --workspace
