#!/bin/bash
# Setup script: starts all containers and wires gateway second-network interfaces.
# Docker reserves .0.1 as the bridge gateway on each network, so our gateways use .0.2.
# Docker Compose can't reliably attach containers to two networks at creation,
# so gateways start on one network, then we attach the second via `docker network connect`.
set -e

COMPOSE="docker compose"
PROJECT="docker"

echo "=== Step 1: Start bootstrap + gateways ==="
$COMPOSE up -d bootstrap nat-gw1 nat-gw2 cgnat-gw home-gw

echo "=== Step 2: Wait for bootstrap health ==="
timeout 60 bash -c 'until docker compose exec -T bootstrap curl -sf http://127.0.0.1:9862/api/v1/health >/dev/null 2>&1; do sleep 2; done'
echo "Bootstrap healthy."

echo "=== Step 3: Wait for gateways to be running ==="
sleep 3
for gw in nat-gw1 nat-gw2 cgnat-gw home-gw; do
    STATUS=$(docker inspect --format '{{.State.Status}}' "${PROJECT}-${gw}-1" 2>/dev/null || echo "missing")
    if [ "$STATUS" != "running" ]; then
        echo "ERROR: $gw is $STATUS"
        docker compose logs "$gw"
        exit 1
    fi
    echo "  $gw: running"
done

echo "=== Step 4: Connect gateways to their second network ==="
docker network connect --ip 10.100.0.20 "${PROJECT}_internet" "${PROJECT}-nat-gw1-1"
echo "  nat-gw1 -> internet (10.100.0.20)"

docker network connect --ip 10.100.0.30 "${PROJECT}_internet" "${PROJECT}-nat-gw2-1"
echo "  nat-gw2 -> internet (10.100.0.30)"

docker network connect --ip 10.100.0.40 "${PROJECT}_internet" "${PROJECT}-cgnat-gw-1"
echo "  cgnat-gw -> internet (10.100.0.40)"

docker network connect --ip 10.103.0.20 "${PROJECT}_isp" "${PROJECT}-home-gw-1"
echo "  home-gw -> isp (10.103.0.20)"

echo "=== Step 5: Fix gateway default routes ==="
# Gateways need their default route to point upstream (toward internet)
# nat-gw1, nat-gw2, cgnat-gw: default via Docker's bridge on internet network is fine
# home-gw: must route via cgnat-gw on the ISP network (not Docker's home bridge)
$COMPOSE exec -T home-gw sh -c 'ip route del default 2>/dev/null; ip route add default via 10.103.0.2'
echo "  home-gw: default via 10.103.0.2 (cgnat-gw)"

echo "=== Step 6: Start dweb nodes ==="
$COMPOSE up -d node1 node2 node3 node4

echo "=== Step 7: Wait for all nodes healthy ==="
TIMEOUT=180
ELAPSED=0
while [ $ELAPSED -lt $TIMEOUT ]; do
    HEALTHY=$($COMPOSE ps --format json | jq -s '[.[] | select(.Health == "healthy")] | length' 2>/dev/null || echo "0")
    if [ "$HEALTHY" -ge 5 ] 2>/dev/null; then
        echo "All nodes healthy ($HEALTHY/5)"
        break
    fi
    echo "  $HEALTHY/5 healthy (${ELAPSED}s)..."
    sleep 5
    ELAPSED=$((ELAPSED+5))
done

if [ $ELAPSED -ge $TIMEOUT ]; then
    echo "ERROR: Timed out waiting for nodes"
    $COMPOSE ps
    $COMPOSE logs --tail=10 node1 node2 node3 node4
    exit 1
fi

echo ""
echo "=== Network topology ready ==="
echo "  bootstrap:  10.100.0.10 (internet)"
echo "  nat-gw1:    10.100.0.20 (internet) / 10.101.0.2 (lan1)"
echo "  nat-gw2:    10.100.0.30 (internet) / 10.102.0.2 (lan2)"
echo "  cgnat-gw:   10.100.0.40 (internet) / 10.103.0.2 (isp)"
echo "  home-gw:    10.103.0.20 (isp)      / 10.104.0.2 (home)"
echo "  node1:      10.101.0.10 (lan1)     -> NAT via nat-gw1"
echo "  node2:      10.101.0.20 (lan1)     -> NAT via nat-gw1"
echo "  node3:      10.102.0.10 (lan2)     -> NAT via nat-gw2"
echo "  node4:      10.104.0.10 (home)     -> CGNAT via home-gw + cgnat-gw"
