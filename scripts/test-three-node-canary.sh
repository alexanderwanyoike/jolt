#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> Running deterministic Alice -> Relay -> Bob canary"
echo "==> Proves Bob discovers Alice's update-log provider through a configured relay/DHT path"

cargo test --locked -p jolt-network bob_discovers_alice_update_log_provider_through_bootstrap_relay -- --nocapture
