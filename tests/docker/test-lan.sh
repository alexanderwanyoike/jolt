#!/bin/bash
# Test Scenario A: LAN Discovery (mDNS)
# node1 and node2 are on the same LAN (lan1)
set -e

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

echo "=== Scenario A: LAN Discovery (mDNS) ==="

# Get node1 and node2 container IPs for API access via docker exec
NODE1="docker compose exec -T node1"
NODE2="docker compose exec -T node2"

# Wait for nodes to be healthy
echo "Waiting for nodes..."
docker compose exec -T node1 curl -sf http://127.0.0.1:9862/api/v1/health > /dev/null || { fail "node1 not healthy"; exit 1; }
docker compose exec -T node2 curl -sf http://127.0.0.1:9862/api/v1/health > /dev/null || { fail "node2 not healthy"; exit 1; }

# Give mDNS time to discover peers on the same LAN
sleep 5

# Test: node1 publishes content
echo "Publishing content on node1..."
CONTENT_ID=$($NODE1 sh -c 'echo "hello from lan1" > /tmp/test.txt && curl -sf -F "file=@/tmp/test.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
if [ -n "$CONTENT_ID" ] && [ "$CONTENT_ID" != "null" ]; then
    pass "node1 published content: $CONTENT_ID"
else
    fail "node1 failed to publish"
    exit 1
fi

# Test: node2 fetches content (should work via mDNS discovery on same LAN)
echo "Fetching content on node2..."
FETCH_SIZE=$($NODE2 sh -c "curl -sf -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CONTENT_ID\"}' | jq -r .size")
if [ "$FETCH_SIZE" = "16" ]; then
    pass "node2 fetched content (16 bytes)"
else
    fail "node2 fetch returned unexpected size: $FETCH_SIZE"
fi

# Test: Both nodes see each other as peers
PEERS1=$($NODE1 sh -c 'curl -sf http://127.0.0.1:9862/api/v1/peers | jq length')
if [ "$PEERS1" -gt 0 ] 2>/dev/null; then
    pass "node1 sees $PEERS1 peer(s)"
else
    fail "node1 sees no peers"
fi

echo ""
echo "LAN Tests: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
