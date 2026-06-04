#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

output="$(./scripts/pastey-two-node-demo.sh --dry-run --base-dir /tmp/jolt-pastey-demo-test --pastey-dir /tmp/pastey)"

grep -q "Alice daemon API: http://127.0.0.1:9871" <<<"$output"
grep -q "Bob daemon API: http://127.0.0.1:9872" <<<"$output"
grep -q "Alice Pastey URL: http://127.0.0.1:5174" <<<"$output"
grep -q "Bob Pastey URL: http://127.0.0.1:5175" <<<"$output"
grep -q "Alice data dir: /tmp/jolt-pastey-demo-test/alice" <<<"$output"
grep -q "Bob data dir: /tmp/jolt-pastey-demo-test/bob" <<<"$output"
grep -q "Bob connects to Alice at /ip4/127.0.0.1/tcp/4901/p2p/<alice-peer-id>" <<<"$output"
grep -q "Optional smoke: app-session approve, Alice publish /pastes/two-node-demo, Bob fetch" <<<"$output"
grep -q "Cleanup: press Ctrl-C to stop spawned daemons and Pastey clients" <<<"$output"

echo "==> Pastey two-node demo harness dry-run test passed"
