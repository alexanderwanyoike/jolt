#!/bin/bash
# Test Scenario B: Simple NAT Traversal
# node1 (behind nat-gw1) and node3 (behind nat-gw2) on different LANs
set -e

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

echo "=== Scenario B: Simple NAT Traversal ==="

NODE1="docker compose exec -T node1"
NODE3="docker compose exec -T node3"

# Wait for nodes and DHT convergence
echo "Waiting for DHT convergence..."
sleep 10

# node1 publishes content
echo "Publishing content on node1..."
CONTENT_ID=$($NODE1 sh -c 'echo "cross-nat content" > /tmp/nat_test.txt && curl -sf -F "file=@/tmp/nat_test.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
if [ -n "$CONTENT_ID" ] && [ "$CONTENT_ID" != "null" ]; then
    pass "node1 published: $CONTENT_ID"
else
    fail "node1 publish failed"
    exit 1
fi

# Wait for DHT provider announcement to propagate
echo "Waiting 60s for DHT provider propagation..."
sleep 60

# node3 fetches (different NAT, must use DHT + relay)
# Retry up to 3 times since DHT propagation may take a while
echo "Fetching on node3 (different NAT)..."
FETCH_SIZE=""
for attempt in 1 2 3; do
    FETCH_RESULT=$($NODE3 sh -c "curl -sf --max-time 45 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CONTENT_ID\"}'")
    FETCH_SIZE=$(echo "$FETCH_RESULT" | jq -r .size 2>/dev/null)
    if [ "$FETCH_SIZE" = "18" ]; then
        break
    fi
    echo "  Attempt $attempt failed (size: $FETCH_SIZE), retrying in 10s..."
    sleep 10
done
if [ "$FETCH_SIZE" = "18" ]; then
    pass "node3 fetched cross-NAT content (18 bytes)"
else
    fail "node3 cross-NAT fetch failed (size: $FETCH_SIZE)"
fi

# Verify node3 status shows connected peers
PEERS3=$($NODE3 sh -c 'curl -sf http://127.0.0.1:9862/api/v1/peers | jq length')
if [ "$PEERS3" -gt 0 ] 2>/dev/null; then
    pass "node3 connected to $PEERS3 peer(s)"
else
    fail "node3 has no peers"
fi

echo ""
echo "NAT Tests: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
