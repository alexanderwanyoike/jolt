#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 OUTPUT_DIRECTORY" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_directory="$1"
mkdir -p "$output_directory"

daemons="${JOLT_LOAD_DAEMONS:-3}"
records="${JOLT_LOAD_RECORDS:-1}"
concurrency="${JOLT_LOAD_CONCURRENCY:-32}"
publish_rate="${JOLT_LOAD_PUBLISH_RATE:-0}"
latency_ms="${JOLT_LOAD_LATENCY_MS:-0}"
bandwidth_kbps="${JOLT_LOAD_BANDWIDTH_KBPS:-0}"
loss_percent="${JOLT_LOAD_LOSS_PERCENT:-0}"
churn_percent="${JOLT_LOAD_CHURN_PERCENT:-0}"
churn_duration_ms="${JOLT_LOAD_CHURN_DURATION_MS:-250}"

for follows in 100 1000 10000; do
  cargo run --release --manifest-path "$repo_root/Cargo.toml" \
    -p jolt-social-graph-load -- \
    --seed 1 \
    --daemons "$daemons" \
    --identities "$follows" \
    --follows "$follows" \
    --records-per-identity "$records" \
    --publish-rate-per-second "$publish_rate" \
    --concurrency "$concurrency" \
    --one-way-latency-ms "$latency_ms" \
    --bandwidth-kbps "$bandwidth_kbps" \
    --loss-percent "$loss_percent" \
    --churn-percent "$churn_percent" \
    --churn-duration-ms "$churn_duration_ms" \
    --json-output "$output_directory/follows-${follows}.json" \
    >"$output_directory/follows-${follows}.stdout.json"
done
