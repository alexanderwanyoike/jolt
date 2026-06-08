#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BIN="$ROOT_DIR/target/debug/jolt"
BASE="$(mktemp -d "${TMPDIR:-/tmp}/jolt-039-process.XXXXXX")"
PIDS=()

cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  rm -rf "$BASE"
}
trap cleanup EXIT

write_config() {
  local node="$1"
  local relay_mode="$2"
  mkdir -p "$BASE/$node/jolt"
  cat > "$BASE/$node/jolt/config.json" <<JSON
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": $relay_mode,
  "home_relay": null
}
JSON
}

wait_for_log() {
  local log="$1"
  local pattern="$2"
  local deadline=$((SECONDS + 15))
  while (( SECONDS < deadline )); do
    if grep -qE "$pattern" "$log" 2>/dev/null; then
      return 0
    fi
    sleep 0.2
  done
  echo "Timed out waiting for '$pattern' in $log" >&2
  tail -n 80 "$log" >&2 || true
  return 1
}

start_node() {
  local node="$1"
  local api_port="$2"
  local p2p_port="$3"
  shift 3

  local log="$BASE/$node.log"
  XDG_DATA_HOME="$BASE/$node" \
    RUST_LOG="jolt=info,jolt_network::node=debug,jolt_server=info" \
    "$BIN" start \
      --api-port "$api_port" \
      --p2p-port "$p2p_port" \
      --transport tcp \
      --no-mdns \
      "$@" > "$log" 2>&1 &
  PIDS+=("$!")
  wait_for_log "$log" "HTTP API: http://127\\.0\\.0\\.1:$api_port"
  wait_for_log "$log" "mDNS discovery disabled"
}

peer_id_from_log() {
  local log="$1"
  sed -n 's/.*Peer ID: //p' "$log" | tail -n 1
}

jolt_address_from_status() {
  local node="$1"
  XDG_DATA_HOME="$BASE/$node" "$BIN" status | sed -n 's/.*Jolt: *//p' | tail -n 1
}

expect_failure_code() {
  local node="$1"
  local address="$2"
  local code="$3"
  local output

  set +e
  output="$(XDG_DATA_HOME="$BASE/$node" "$BIN" resolve "$address" 2>&1)"
  local status=$?
  set -e

  echo "$output"
  if [[ "$status" -eq 0 ]]; then
    echo "Expected resolve to fail with $code but it succeeded" >&2
    exit 1
  fi
  if ! grep -q "$code:" <<<"$output"; then
    echo "Expected resolve failure code $code" >&2
    exit 1
  fi
}

echo "==> Building jolt debug binary"
cargo build --locked -p jolt-node >/dev/null

echo "==> Preparing isolated process homes under $BASE"
write_config isolated false
write_config dead-relay true
write_config unreachable false
write_config r2 true
write_config tim false

echo "==> No bootstrap relays gives a no_bootstrap_relays code"
start_node isolated 9941 4941
isolated_address="$(jolt_address_from_status isolated)/missing"
expect_failure_code isolated "$isolated_address" "no_bootstrap_relays"

echo "==> Unreachable configured relay gives a relay_unreachable code"
start_node dead-relay 9942 4942
dead_peer="$(peer_id_from_log "$BASE/dead-relay.log")"
dead_addr="/ip4/127.0.0.1/tcp/4942/p2p/$dead_peer"
dead_pid="${PIDS[-1]}"
kill "$dead_pid" 2>/dev/null || true
wait "$dead_pid" 2>/dev/null || true
PIDS=("${PIDS[@]:0:${#PIDS[@]}-1}")
start_node unreachable 9943 4943 --bootstrap "$dead_addr"
expect_failure_code unreachable "$isolated_address" "relay_unreachable"

echo "==> Reachable relay mesh with unknown identity gives an identity_provider_not_found code"
start_node r2 9944 4944
r2_peer="$(peer_id_from_log "$BASE/r2.log")"
r2_addr="/ip4/127.0.0.1/tcp/4944/p2p/$r2_peer"
start_node tim 9945 4945 --bootstrap "$r2_addr"
expect_failure_code tim "$isolated_address" "identity_provider_not_found"

if grep -R "mDNS discovered peer" "$BASE"/*.log; then
  echo "mDNS discovery leaked into the process test" >&2
  exit 1
fi

echo "==> Relay discovery failure UX process test passed"
