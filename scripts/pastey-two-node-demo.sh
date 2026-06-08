#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

BIN="$ROOT_DIR/target/debug/jolt"
BASE_DIR=""
PASTEY_DIR="$ROOT_DIR/../jolt-apps/pastey"
DRY_RUN=0
SMOKE=0
START_PASTEY=1
KEEP_DATA=0
PIDS=()

ALICE_API_PORT=9871
ALICE_P2P_PORT=4901
ALICE_PASTEY_PORT=5174
BOB_API_PORT=9872
BOB_P2P_PORT=4902
BOB_PASTEY_PORT=5175
PASTE_PATH="/pastes/two-node-demo"
PASTE_TEXT="hello from Alice Pastey through the two-node harness"

usage() {
  cat <<'USAGE'
Usage: scripts/pastey-two-node-demo.sh [options]

Starts a local Alice/Bob Jolt + Pastey demo on one machine.

Options:
  --dry-run              Print the plan without starting processes.
  --smoke                Run an app-session publish/fetch smoke and exit.
  --no-pastey            Start only the daemons; useful with --smoke.
  --base-dir DIR         Use a specific working directory.
  --pastey-dir DIR       Path to the Pastey repo.
  --keep-data            Do not delete the working directory on exit.
  -h, --help             Show this help.
USAGE
}

while (($#)); do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --smoke)
      SMOKE=1
      shift
      ;;
    --no-pastey)
      START_PASTEY=0
      shift
      ;;
    --base-dir)
      if [[ $# -lt 2 ]]; then
        echo "--base-dir requires DIR" >&2
        exit 2
      fi
      BASE_DIR="$2"
      shift 2
      ;;
    --pastey-dir)
      if [[ $# -lt 2 ]]; then
        echo "--pastey-dir requires DIR" >&2
        exit 2
      fi
      PASTEY_DIR="$2"
      shift 2
      ;;
    --keep-data)
      KEEP_DATA=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$BASE_DIR" ]]; then
  if [[ "$DRY_RUN" -eq 1 ]]; then
    BASE_DIR="${TMPDIR:-/tmp}/jolt-pastey-demo.<temp>"
  else
    BASE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/jolt-pastey-demo.XXXXXX")"
  fi
fi

ALICE_DIR="$BASE_DIR/alice"
BOB_DIR="$BASE_DIR/bob"
ALICE_LOG="$BASE_DIR/alice.log"
BOB_LOG="$BASE_DIR/bob.log"
ALICE_PASTEY_LOG="$BASE_DIR/alice-pastey.log"
BOB_PASTEY_LOG="$BASE_DIR/bob-pastey.log"

cleanup() {
  for pid in "${PIDS[@]:-}"; do
    kill "$pid" 2>/dev/null || true
  done
  wait 2>/dev/null || true
  if [[ "$KEEP_DATA" -eq 0 && "$DRY_RUN" -eq 0 ]]; then
    rm -rf "$BASE_DIR"
  fi
}
trap cleanup EXIT INT TERM

print_plan() {
  cat <<PLAN
Pastey two-node local demo plan

Working dir: $BASE_DIR
Alice data dir: $ALICE_DIR
Bob data dir: $BOB_DIR

Alice daemon API: http://127.0.0.1:$ALICE_API_PORT
Alice daemon P2P: /ip4/127.0.0.1/tcp/$ALICE_P2P_PORT
Bob daemon API: http://127.0.0.1:$BOB_API_PORT
Bob daemon P2P: /ip4/127.0.0.1/tcp/$BOB_P2P_PORT
Bob connects to Alice at /ip4/127.0.0.1/tcp/$ALICE_P2P_PORT/p2p/<alice-peer-id>

Alice Pastey URL: http://127.0.0.1:$ALICE_PASTEY_PORT
Bob Pastey URL: http://127.0.0.1:$BOB_PASTEY_PORT
Pastey repo: $PASTEY_DIR

Optional smoke: app-session approve, Alice publish $PASTE_PATH, Bob fetch
Cleanup: press Ctrl-C to stop spawned daemons and Pastey clients
PLAN
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

ensure_port_free() {
  local port="$1"
  if command -v lsof >/dev/null 2>&1 && lsof -iTCP:"$port" -sTCP:LISTEN -n -P >/dev/null 2>&1; then
    echo "Port $port is already in use" >&2
    exit 1
  fi
}

write_config() {
  local node_dir="$1"
  mkdir -p "$node_dir/jolt"
  cat > "$node_dir/jolt/config.json" <<JSON
{
  "bootstrap_relays": [],
  "use_builtin_bootstrap_relays": false,
  "bootstrap_relay": false,
  "home_relay": null
}
JSON
}

wait_for_log() {
  local log="$1"
  local pattern="$2"
  local deadline=$((SECONDS + 20))
  while (( SECONDS < deadline )); do
    if grep -qE "$pattern" "$log" 2>/dev/null; then
      return 0
    fi
    sleep 0.2
  done
  echo "Timed out waiting for '$pattern' in $log" >&2
  tail -n 120 "$log" >&2 || true
  return 1
}

wait_for_http() {
  local url="$1"
  local deadline=$((SECONDS + 20))
  while (( SECONDS < deadline )); do
    if curl -fsS "$url" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.2
  done
  echo "Timed out waiting for $url" >&2
  return 1
}

status_json() {
  local api_port="$1"
  curl -fsS "http://127.0.0.1:$api_port/api/v1/status"
}

peer_id() {
  local api_port="$1"
  status_json "$api_port" | jq -r '.peer_id'
}

identity_address() {
  local api_port="$1"
  status_json "$api_port" | jq -r '.identity_address'
}

start_daemon() {
  local label="$1"
  local data_dir="$2"
  local api_port="$3"
  local p2p_port="$4"
  local log="$5"

  XDG_DATA_HOME="$data_dir" \
    RUST_LOG="jolt=info,jolt_network::node=info,jolt_server=info" \
    "$BIN" start \
      --transport tcp \
      --p2p-port "$p2p_port" \
      --api-port "$api_port" \
      --no-bootstrap \
      --no-mdns > "$log" 2>&1 &
  PIDS+=("$!")
  wait_for_log "$log" "HTTP API: http://127\\.0\\.0\\.1:$api_port"
  wait_for_http "http://127.0.0.1:$api_port/api/v1/status"
  echo "==> $label daemon ready on http://127.0.0.1:$api_port"
}

connect_bob_to_alice() {
  local alice_peer
  alice_peer="$(peer_id "$ALICE_API_PORT")"
  local alice_addr="/ip4/127.0.0.1/tcp/$ALICE_P2P_PORT/p2p/$alice_peer"

  curl -fsS \
    -X POST "http://127.0.0.1:$BOB_API_PORT/api/v1/peers/connect" \
    -H "Content-Type: application/json" \
    -d "{\"multiaddr\":\"$alice_addr\"}" >/dev/null

  local deadline=$((SECONDS + 10))
  while (( SECONDS < deadline )); do
    if [[ "$(status_json "$BOB_API_PORT" | jq -r '.connected_peers')" -gt 0 ]]; then
      echo "==> Bob connected to Alice at $alice_addr"
      return 0
    fi
    sleep 0.2
  done
  echo "Timed out waiting for Bob to connect to Alice" >&2
  tail -n 120 "$BOB_LOG" >&2 || true
  return 1
}

start_pastey() {
  local label="$1"
  local daemon_url="$2"
  local port="$3"
  local log="$4"

  if [[ ! -d "$PASTEY_DIR" ]]; then
    echo "Pastey repo not found at $PASTEY_DIR" >&2
    echo "Run with --pastey-dir /path/to/pastey or clone https://github.com/alexanderwanyoike/pastey" >&2
    exit 1
  fi

  (
    cd "$PASTEY_DIR"
    VITE_JOLT_DAEMON_URL="$daemon_url" npm run dev -- --host 127.0.0.1 --port "$port"
  ) > "$log" 2>&1 &
  PIDS+=("$!")
  wait_for_log "$log" "Local:.*http://127\\.0\\.0\\.1:$port"
  echo "==> $label Pastey ready at http://127.0.0.1:$port"
}

approve_session() {
  local api_port="$1"
  local app_id="$2"
  local app_name="$3"
  local app_origin="$4"
  local identity="$5"
  local capabilities='["resolve:public","fetch:public","publish:/pastes/*","inventory:/pastes/*","pin:own:/pastes/*"]'

  local request_json
  request_json="$(
    jq -n \
      --arg app_id "$app_id" \
      --arg app_name "$app_name" \
      --arg app_origin "$app_origin" \
      --arg identity "$identity" \
      --argjson capabilities "$capabilities" \
      '{
        app_id: $app_id,
        app_name: $app_name,
        app_origin: $app_origin,
        requested_identity: $identity,
        requested_capabilities: $capabilities
      }'
  )"

  local request_id
  request_id="$(
    curl -fsS \
      -X POST "http://127.0.0.1:$api_port/app/v1/sessions/request" \
      -H "Content-Type: application/json" \
      -d "$request_json" | jq -r '.request_id'
  )"

  jq -n \
    --arg identity "$identity" \
    --argjson capabilities "$capabilities" \
    '{ identity: $identity, capabilities: $capabilities, expires_at: null }' |
    curl -fsS \
      -X POST "http://127.0.0.1:$api_port/admin/v1/app-requests/$request_id/approve" \
      -H "Content-Type: application/json" \
      -d @- | jq -r '.session_token'
}

run_smoke() {
  echo "==> Running app-session smoke"
  local alice_identity bob_identity alice_token bob_token sample_file publish_json address content_id fetch_json fetched_content_id

  alice_identity="$(identity_address "$ALICE_API_PORT")"
  bob_identity="$(identity_address "$BOB_API_PORT")"

  alice_token="$(
    approve_session \
      "$ALICE_API_PORT" \
      "pastey.demo.alice" \
      "Pastey Alice" \
      "http://127.0.0.1:$ALICE_PASTEY_PORT" \
      "$alice_identity"
  )"
  bob_token="$(
    approve_session \
      "$BOB_API_PORT" \
      "pastey.demo.bob" \
      "Pastey Bob" \
      "http://127.0.0.1:$BOB_PASTEY_PORT" \
      "$bob_identity"
  )"

  sample_file="$BASE_DIR/alice-paste.txt"
  printf '%s\n' "$PASTE_TEXT" > "$sample_file"
  publish_json="$(
    curl -fsS \
      -X POST "http://127.0.0.1:$ALICE_API_PORT/app/v1/publish" \
      -H "Authorization: Bearer $alice_token" \
      -F "file=@$sample_file;filename=two-node-demo.txt" \
      -F "path=$PASTE_PATH"
  )"
  address="$(printf '%s' "$publish_json" | jq -r '.address')"
  content_id="$(printf '%s' "$publish_json" | jq -r '.content_id')"

  local deadline=$((SECONDS + 10))
  while (( SECONDS < deadline )); do
    set +e
    fetch_json="$(
      curl -fsS \
        -X POST "http://127.0.0.1:$BOB_API_PORT/app/v1/fetch" \
        -H "Authorization: Bearer $bob_token" \
        -H "Content-Type: application/json" \
        -d "{\"target\":\"$address\"}" 2>/dev/null
    )"
    local fetch_status=$?
    set -e
    if [[ "$fetch_status" -eq 0 ]]; then
      fetched_content_id="$(printf '%s' "$fetch_json" | jq -r '.content_id')"
      if [[ "$fetched_content_id" == "$content_id" ]]; then
        echo "==> Sample paste address: $address"
        echo "==> Bob fetched Alice paste through app API"
        return 0
      fi
    fi
    sleep 0.4
  done

  echo "Bob did not fetch Alice's sample paste before timeout" >&2
  echo "Alice address: $address" >&2
  tail -n 120 "$BOB_LOG" >&2 || true
  return 1
}

if [[ "$DRY_RUN" -eq 1 ]]; then
  print_plan
  exit 0
fi

require_command cargo
require_command curl
require_command jq
if [[ "$START_PASTEY" -eq 1 ]]; then
  require_command npm
fi

for port in "$ALICE_API_PORT" "$ALICE_P2P_PORT" "$BOB_API_PORT" "$BOB_P2P_PORT"; do
  ensure_port_free "$port"
done
if [[ "$START_PASTEY" -eq 1 ]]; then
  for port in "$ALICE_PASTEY_PORT" "$BOB_PASTEY_PORT"; do
    ensure_port_free "$port"
  done
fi

mkdir -p "$BASE_DIR"
write_config "$ALICE_DIR"
write_config "$BOB_DIR"

print_plan

echo "==> Building jolt debug binary"
cargo build --locked -p jolt-node >/dev/null

echo "==> Starting Alice and Bob daemons"
start_daemon "Alice" "$ALICE_DIR" "$ALICE_API_PORT" "$ALICE_P2P_PORT" "$ALICE_LOG"
start_daemon "Bob" "$BOB_DIR" "$BOB_API_PORT" "$BOB_P2P_PORT" "$BOB_LOG"
connect_bob_to_alice

echo "==> Alice identity: $(identity_address "$ALICE_API_PORT")"
echo "==> Bob identity: $(identity_address "$BOB_API_PORT")"

if [[ "$START_PASTEY" -eq 1 ]]; then
  echo "==> Starting Pastey clients"
  start_pastey "Alice" "http://127.0.0.1:$ALICE_API_PORT" "$ALICE_PASTEY_PORT" "$ALICE_PASTEY_LOG"
  start_pastey "Bob" "http://127.0.0.1:$BOB_API_PORT" "$BOB_PASTEY_PORT" "$BOB_PASTEY_LOG"
fi

if [[ "$SMOKE" -eq 1 ]]; then
  run_smoke
  exit 0
fi

cat <<READY

Pastey two-node demo is running.

Alice Pastey URL: http://127.0.0.1:$ALICE_PASTEY_PORT
Bob Pastey URL: http://127.0.0.1:$BOB_PASTEY_PORT

Approve Pastey requests in Jolt Console or use --smoke for an automated app-API sample.
Logs and isolated data are under $BASE_DIR while this process is running.
Cleanup: press Ctrl-C to stop spawned daemons and Pastey clients
READY

while true; do
  sleep 3600
done
