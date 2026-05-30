#!/bin/bash
# Test Scenario C: CGNAT (Double NAT)
# node4 is behind two layers of NAT (home-gw + cgnat-gw)
set -e

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

echo "=== Scenario C: CGNAT (Double NAT) ==="

NODE1="docker compose exec -T node1"
NODE4="docker compose exec -T node4"

# Wait for DHT convergence (CGNAT nodes take longer)
echo "Waiting for CGNAT node to join DHT..."
sleep 15

# node1 publishes content
echo "Publishing content on node1..."
CONTENT_ID=$($NODE1 sh -c 'echo "cgnat test data" > /tmp/cgnat_test.txt && curl -sf -F "file=@/tmp/cgnat_test.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
if [ -n "$CONTENT_ID" ] && [ "$CONTENT_ID" != "null" ]; then
    pass "node1 published: $CONTENT_ID"
else
    fail "node1 publish failed"
    exit 1
fi

sleep 5

# node4 fetches (behind double NAT)
echo "Fetching on node4 (behind CGNAT)..."
FETCH_RESULT=$($NODE4 sh -c "curl -sf --max-time 60 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CONTENT_ID\"}'")
FETCH_SIZE=$(echo "$FETCH_RESULT" | jq -r .size 2>/dev/null)
if [ "$FETCH_SIZE" = "16" ]; then
    pass "node4 fetched through CGNAT (16 bytes)"
else
    fail "node4 CGNAT fetch failed (size: $FETCH_SIZE)"
fi

# Verify node4 has peers (connected via relay)
PEERS4=$($NODE4 sh -c 'curl -sf http://127.0.0.1:9862/api/v1/peers | jq length')
if [ "$PEERS4" -gt 0 ] 2>/dev/null; then
    pass "node4 connected to $PEERS4 peer(s) through CGNAT"
else
    fail "node4 has no peers through CGNAT"
fi

echo ""
echo "CGNAT Tests: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
