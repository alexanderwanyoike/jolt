#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BIN="$ROOT_DIR/target/debug/jolt"
BASE="$(mktemp -d "${TMPDIR:-/tmp}/jolt-037-process.XXXXXX")"
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

echo "==> Building jolt debug binary"
cargo build --locked -p jolt-node >/dev/null

echo "==> Preparing isolated process homes under $BASE"
write_config r1 true
write_config r2 true
write_config r3 true
write_config r4 true
write_config tim false

echo "==> Starting relay chain with mDNS disabled"
start_node r4 9924 4924
r4_peer="$(peer_id_from_log "$BASE/r4.log")"
r4_addr="/ip4/127.0.0.1/tcp/4924/p2p/$r4_peer"

start_node r3 9923 4923 --bootstrap "$r4_addr"
r3_peer="$(peer_id_from_log "$BASE/r3.log")"
r3_addr="/ip4/127.0.0.1/tcp/4923/p2p/$r3_peer"

start_node r2 9922 4922 --bootstrap "$r3_addr"
r2_peer="$(peer_id_from_log "$BASE/r2.log")"
r2_addr="/ip4/127.0.0.1/tcp/4922/p2p/$r2_peer"

start_node r1 9921 4921 --bootstrap "$r2_addr"
r1_peer="$(peer_id_from_log "$BASE/r1.log")"
r1_addr="/ip4/127.0.0.1/tcp/4921/p2p/$r1_peer"

start_node tim 9920 4920 --bootstrap "$r1_addr"

echo "==> Publishing Alice content on R4"
printf 'hello from card 037 process test\n' > "$BASE/alice-post.txt"
publish_output="$(
  XDG_DATA_HOME="$BASE/r4" "$BIN" publish "$BASE/alice-post.txt" --path /demo/card037
)"
echo "$publish_output"
address="$(printf '%s\n' "$publish_output" | grep -E '^[a-z2-7]+\.jolt/' | tail -n 1)"

echo "==> Resolving from Tim through the relay chain"
resolve_output="$(XDG_DATA_HOME="$BASE/tim" "$BIN" resolve "$address")"
echo "$resolve_output"
printf '%s\n' "$resolve_output" | grep -q "Source: network"

echo "==> Fetching from Tim"
XDG_DATA_HOME="$BASE/tim" "$BIN" fetch "$address" -o "$BASE/tim-fetched.txt"
diff -u "$BASE/alice-post.txt" "$BASE/tim-fetched.txt"

echo "==> Verifying process logs show relay query forwarding and no mDNS discovery"
grep -q "Forwarding identity provider query" "$BASE/r1.log"
grep -q "Forwarding identity provider query" "$BASE/r2.log"
grep -q "Forwarding identity provider query" "$BASE/r3.log"
grep -q "Received .* identity provider candidates" "$BASE/tim.log"
if grep -R "mDNS discovered peer" "$BASE"/*.log; then
  echo "mDNS discovery leaked into the process test" >&2
  exit 1
fi

echo "==> Identity provider query forwarding process test passed"
