#!/bin/bash
# Node entrypoint: sets up routing, discovers bootstrap peer ID, starts jolt
set -e

GATEWAY_IP="$1"
PORT="${2:-4001}"
BOOTSTRAP_IP="${3:-10.100.0.10}"
BOOTSTRAP_PORT="${4:-4001}"

# Set default route through the NAT gateway
ip route del default 2>/dev/null || true
ip route add default via "$GATEWAY_IP"

# Wait for bootstrap to be reachable and get its peer ID
echo "Waiting for bootstrap at $BOOTSTRAP_IP..."
PEER_ID=""
for i in $(seq 1 30); do
    PEER_ID=$(curl -sf --max-time 3 "http://${BOOTSTRAP_IP}:9862/api/v1/status" | jq -r .peer_id 2>/dev/null || true)
    if [ -n "$PEER_ID" ] && [ "$PEER_ID" != "null" ]; then
        break
    fi
    sleep 2
done

if [ -z "$PEER_ID" ] || [ "$PEER_ID" = "null" ]; then
    echo "ERROR: Could not discover bootstrap peer ID"
    exit 1
fi

BOOTSTRAP_ADDR="/ip4/${BOOTSTRAP_IP}/udp/${BOOTSTRAP_PORT}/quic-v1/p2p/${PEER_ID}"
echo "Bootstrap: $BOOTSTRAP_ADDR"

exec jolt start --p2p-port "$PORT" --api-port 9862 --api-bind 0.0.0.0 --bootstrap "$BOOTSTRAP_ADDR"
