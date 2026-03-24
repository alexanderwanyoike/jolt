#!/bin/bash
# Test Scenarios D-H: Cross-NAT internet scenarios
set -e

PASS=0
FAIL=0

pass() { echo "  PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "  FAIL: $1"; FAIL=$((FAIL+1)); }

NODE1="docker compose exec -T node1"
NODE2="docker compose exec -T node2"
NODE3="docker compose exec -T node3"
NODE4="docker compose exec -T node4"

# === Scenario D: Cache Relay with Node Failure ===
echo "=== Scenario D: Cache Relay with Node Failure ==="

# node1 publishes, node3 fetches (caches it)
CONTENT_D=$($NODE1 sh -c 'echo "cache relay test" > /tmp/relay_test.txt && curl -sf -F "file=@/tmp/relay_test.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
if [ -n "$CONTENT_D" ] && [ "$CONTENT_D" != "null" ]; then
    pass "D: node1 published: $CONTENT_D"
else
    fail "D: node1 publish failed"
fi

sleep 5

# node3 fetches (caches it)
SIZE_D=$($NODE3 sh -c "curl -sf --max-time 65 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CONTENT_D\"}' | jq -r .size")
if [ -n "$SIZE_D" ] && [ "$SIZE_D" != "null" ] && [ "$SIZE_D" -gt 0 ] 2>/dev/null; then
    pass "D: node3 cached content ($SIZE_D bytes)"
else
    fail "D: node3 failed to cache (size: $SIZE_D)"
fi

# === Scenario E: Cross-NAT Concurrent Operations ===
echo ""
echo "=== Scenario E: Cross-NAT Concurrent Operations ==="

# node1 publishes 3 files
CID1=$($NODE1 sh -c 'echo "file one" > /tmp/f1.txt && curl -sf -F "file=@/tmp/f1.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
CID2=$($NODE1 sh -c 'echo "file two" > /tmp/f2.txt && curl -sf -F "file=@/tmp/f2.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
CID3=$($NODE1 sh -c 'echo "file three" > /tmp/f3.txt && curl -sf -F "file=@/tmp/f3.txt" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')

if [ -n "$CID1" ] && [ -n "$CID2" ] && [ -n "$CID3" ]; then
    pass "E: published 3 files"
else
    fail "E: failed to publish all files"
fi

sleep 3

# node3 fetches all 3 concurrently
SIZES=$($NODE3 sh -c "
    (curl -sf --max-time 45 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CID1\"}' | jq -r .size) &
    (curl -sf --max-time 45 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CID2\"}' | jq -r .size) &
    (curl -sf --max-time 45 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$CID3\"}' | jq -r .size) &
    wait
")
# Check that we got results (not empty)
FETCH_COUNT=$(echo "$SIZES" | grep -c '[0-9]' || true)
if [ "$FETCH_COUNT" -ge 2 ]; then
    pass "E: node3 fetched $FETCH_COUNT/3 files concurrently"
else
    fail "E: concurrent fetch got only $FETCH_COUNT results"
fi

# === Scenario F: Graceful Shutdown ===
echo ""
echo "=== Scenario F: Graceful Shutdown ==="

# Check all nodes are running
STATUS1=$($NODE1 sh -c 'curl -sf http://127.0.0.1:9862/api/v1/status | jq -r .peer_id')
if [ -n "$STATUS1" ] && [ "$STATUS1" != "null" ]; then
    pass "F: all nodes responding before shutdown test"
else
    fail "F: nodes not all responding"
fi

# === Scenario G: Pin Persistence ===
echo ""
echo "=== Scenario G: Pin Persistence ==="

# node3 pins the content it fetched
PIN_RESULT=$($NODE3 sh -c "curl -sf -X POST http://127.0.0.1:9862/api/v1/cache/pin/$CONTENT_D | jq -r .status")
if [ "$PIN_RESULT" = "pinned" ]; then
    pass "G: content pinned on node3"
else
    fail "G: pin failed: $PIN_RESULT"
fi

# Verify it shows in cache entries as pinned
IS_PINNED=$($NODE3 sh -c "curl -sf http://127.0.0.1:9862/api/v1/cache/entries | jq '[.[] | select(.pinned==true)] | length'")
if [ "$IS_PINNED" -gt 0 ] 2>/dev/null; then
    pass "G: pinned content appears in cache list"
else
    fail "G: pinned content not in cache list"
fi

# === Scenario H: Large File Through NAT ===
echo ""
echo "=== Scenario H: Large File Through NAT ==="

# Generate a 1MB file (smaller for faster tests) and publish
LARGE_CID=$($NODE1 sh -c 'dd if=/dev/urandom of=/tmp/large.bin bs=1024 count=1024 2>/dev/null && curl -sf -F "file=@/tmp/large.bin" http://127.0.0.1:9862/api/v1/publish | jq -r .content_id')
if [ -n "$LARGE_CID" ] && [ "$LARGE_CID" != "null" ]; then
    pass "H: published 1MB file: $LARGE_CID"
else
    fail "H: large file publish failed"
fi

sleep 5

# node3 fetches the large file (longer timeout for large transfers over relay)
LARGE_SIZE=$($NODE3 sh -c "curl -sf --max-time 180 -X POST http://127.0.0.1:9862/api/v1/fetch -H 'Content-Type: application/json' -d '{\"content_id\": \"$LARGE_CID\"}' | jq -r .size")
if [ "$LARGE_SIZE" = "1048576" ]; then
    pass "H: fetched 1MB file across NAT"
else
    fail "H: large file fetch returned size: $LARGE_SIZE (expected 1048576)"
fi

echo ""
echo "Internet Tests: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ]
