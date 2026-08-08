#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BIN="${JOLT_BIN:-$ROOT_DIR/target/debug/jolt}"
BASE="$(mktemp -d "${TMPDIR:-/tmp}/jolt-relay-pin-policy.XXXXXX")"
declare -A NODE_PIDS=()

cleanup() {
  for pid in "${NODE_PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  rm -rf "$BASE"
}
trap cleanup EXIT INT TERM

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

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

wait_for_http() {
  local port="$1"
  local log="$2"
  local deadline=$((SECONDS + 20))
  while (( SECONDS < deadline )); do
    if curl -fsS "http://127.0.0.1:$port/api/v1/status" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.2
  done
  echo "Timed out waiting for daemon API on port $port" >&2
  tail -n 100 "$log" >&2 || true
  return 1
}

start_node() {
  local node="$1"
  local api_port="$2"
  local p2p_port="$3"
  local log="$4"
  shift 4

  XDG_DATA_HOME="$BASE/$node" \
    RUST_LOG="jolt=info,jolt_network::node=info,jolt_server=info" \
    "$BIN" start \
      --api-port "$api_port" \
      --p2p-port "$p2p_port" \
      --transport tcp \
      --no-mdns \
      "$@" > "$log" 2>&1 &
  NODE_PIDS["$node"]=$!
  wait_for_http "$api_port" "$log"
}

stop_node() {
  local node="$1"
  kill "${NODE_PIDS[$node]}" 2>/dev/null || true
  wait "${NODE_PIDS[$node]}" 2>/dev/null || true
  unset 'NODE_PIDS[$node]'
  rm -f "$BASE/$node/jolt/daemon.json"
}

status_field() {
  local port="$1"
  local field="$2"
  curl -fsS "http://127.0.0.1:$port/api/v1/status" | jq -r ".$field"
}

wait_for_peer() {
  local port="$1"
  local deadline=$((SECONDS + 15))
  while (( SECONDS < deadline )); do
    if (( $(status_field "$port" connected_peers) > 0 )); then
      return 0
    fi
    sleep 0.2
  done
  echo "Timed out waiting for a peer connection on port $port" >&2
  return 1
}

configure_home_relay() {
  local node="$1"
  local relay_addr="$2"
  local relay_api_port="$3"
  XDG_DATA_HOME="$BASE/$node" "$BIN" home-relay set "$relay_addr" \
    --capability pinning \
    --api-url "http://127.0.0.1:$relay_api_port" >/dev/null
}

publish() {
  local node="$1"
  local file="$2"
  local path="$3"
  XDG_DATA_HOME="$BASE/$node" "$BIN" publish "$file" --path "$path"
}

require_command curl
require_command jq
if [[ ! -x "$BIN" ]]; then
  echo "Jolt binary is missing: $BIN" >&2
  echo "Build it once or set JOLT_BIN to a PR binary." >&2
  exit 1
fi

RELAY_API=9980
RELAY_P2P=4980
ALICE_API=9981
ALICE_P2P=4981
MALLORY_API=9982
MALLORY_P2P=4982
BOB_API=9983
BOB_P2P=4983

echo "==> Preparing isolated relay, Alice, Mallory and Bob homes"
write_config relay true
write_config alice false
write_config mallory false
write_config bob false

echo "==> Generating Alice's persistent identity"
start_node alice "$ALICE_API" "$ALICE_P2P" "$BASE/alice-initial.log" --no-bootstrap
alice_identity="$(status_field "$ALICE_API" identity_address)"
stop_node alice

echo "==> Starting a default-deny relay with Alice allowlisted"
start_node relay "$RELAY_API" "$RELAY_P2P" "$BASE/relay.log" \
  --no-bootstrap \
  --pin-allow "$alice_identity"
relay_peer="$(status_field "$RELAY_API" peer_id)"
relay_addr="/ip4/127.0.0.1/tcp/$RELAY_P2P/p2p/$relay_peer"

configure_home_relay alice "$relay_addr" "$RELAY_API"
configure_home_relay mallory "$relay_addr" "$RELAY_API"

start_node alice "$ALICE_API" "$ALICE_P2P" "$BASE/alice.log" --bootstrap "$relay_addr"
start_node mallory "$MALLORY_API" "$MALLORY_P2P" "$BASE/mallory.log" --bootstrap "$relay_addr"
wait_for_peer "$ALICE_API"
wait_for_peer "$MALLORY_API"

echo "alice relay-backed content" > "$BASE/alice.txt"
alice_publish="$(publish alice "$BASE/alice.txt" /canary/alice)"
alice_cid="$(printf '%s\n' "$alice_publish" | grep -E '^b[a-z2-7]+$' | tail -n 1)"
alice_address="$(printf '%s\n' "$alice_publish" | grep -E '^[a-z2-7]+\.jolt/' | tail -n 1)"

echo "==> Proving the allowlisted owner can pin"
XDG_DATA_HOME="$BASE/alice" "$BIN" home-relay pin "$alice_cid" >/dev/null

echo "mallory must not consume relay storage" > "$BASE/mallory.txt"
mallory_publish="$(publish mallory "$BASE/mallory.txt" /canary/mallory)"
mallory_cid="$(printf '%s\n' "$mallory_publish" | grep -E '^b[a-z2-7]+$' | tail -n 1)"
if XDG_DATA_HOME="$BASE/mallory" "$BIN" home-relay pin "$mallory_cid" \
  > "$BASE/mallory-pin.log" 2>&1; then
  echo "Non-allowlisted Mallory unexpectedly pinned content" >&2
  exit 1
fi
grep -q "identity is not allowlisted" "$BASE/mallory-pin.log"
if XDG_DATA_HOME="$BASE/relay" "$BIN" cache list | grep -q "$mallory_cid"; then
  echo "Relay fetched Mallory's rejected content" >&2
  exit 1
fi

echo "==> Proving Alice can go offline while Bob fetches from the relay"
stop_node alice
start_node bob "$BOB_API" "$BOB_P2P" "$BASE/bob.log" --bootstrap "$relay_addr"
wait_for_peer "$BOB_API"
XDG_DATA_HOME="$BASE/bob" "$BIN" fetch "$alice_address" -o "$BASE/bob-fetched.txt" >/dev/null
diff -u "$BASE/alice.txt" "$BASE/bob-fetched.txt"
bob_identity="$(status_field "$BOB_API" identity_address)"

echo "==> Restarting the relay while adding Bob to the persisted allowlist"
stop_node relay
start_node relay "$RELAY_API" "$RELAY_P2P" "$BASE/relay-restarted.log" \
  --no-bootstrap \
  --pin-allow "$bob_identity"

start_node alice "$ALICE_API" "$ALICE_P2P" "$BASE/alice-restarted.log" --bootstrap "$relay_addr"
wait_for_peer "$ALICE_API"
echo "alice remains allowed after policy update" > "$BASE/alice-second.txt"
alice_second_publish="$(publish alice "$BASE/alice-second.txt" /canary/alice-second)"
alice_second_cid="$(printf '%s\n' "$alice_second_publish" | grep -E '^b[a-z2-7]+$' | tail -n 1)"
XDG_DATA_HOME="$BASE/alice" "$BIN" home-relay pin "$alice_second_cid" >/dev/null

echo "==> Relay pin allowlist process canary passed"
