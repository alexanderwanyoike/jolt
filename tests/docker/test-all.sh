#!/bin/bash
# Run all dweb Docker network simulation tests
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "================================================"
echo "  dweb Docker Network Simulation Tests"
echo "================================================"
echo ""

# Build images
echo "Building Docker images..."
docker compose build

# Set up the full network topology
echo ""
bash setup.sh

# Give DHT time to converge across NAT boundaries
echo ""
echo "Waiting for DHT convergence (20s)..."
sleep 20

TOTAL_FAIL=0

run_test() {
    local test_name="$1"
    local test_script="$2"
    echo ""
    echo "Running: $test_name"
    echo "----------------------------------------"
    if bash "$test_script"; then
        echo "  => $test_name: ALL PASSED"
    else
        echo "  => $test_name: SOME FAILURES"
        TOTAL_FAIL=$((TOTAL_FAIL+1))
    fi
}

run_test "LAN Discovery" "$SCRIPT_DIR/test-lan.sh"
run_test "NAT Traversal" "$SCRIPT_DIR/test-nat.sh"
run_test "CGNAT (Double NAT)" "$SCRIPT_DIR/test-cgnat.sh"
run_test "Internet Scenarios" "$SCRIPT_DIR/test-internet.sh"

echo ""
echo "================================================"
if [ $TOTAL_FAIL -eq 0 ]; then
    echo "  ALL TEST SUITES PASSED"
else
    echo "  $TOTAL_FAIL TEST SUITE(S) FAILED"
fi
echo "================================================"

# Cleanup
echo ""
echo "Cleaning up..."
docker compose down -v

exit $TOTAL_FAIL
