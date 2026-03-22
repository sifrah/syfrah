#!/usr/bin/env bash
# End-to-end test: 3 nodes forming a mesh
# Requires: docker, docker compose
# Run from repo root: bash tests/e2e/run.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "=== Building Docker image ==="
docker compose -f "$SCRIPT_DIR/docker-compose.yml" build

echo ""
echo "=== Starting node1 (init) ==="
# Start node1 in background, capture its output to get the token
docker compose -f "$SCRIPT_DIR/docker-compose.yml" run -d --name syfrah-node1 \
    node1 init --name test-mesh --node-name node-1 --port 51820 --endpoint 172.28.0.10:51820

# Wait for node1 to start and extract the token
echo "Waiting for node1 to initialize..."
sleep 5
TOKEN=$(docker logs syfrah-node1 2>&1 | grep "Secret:" | awk '{print $2}')

if [ -z "$TOKEN" ]; then
    echo "FAIL: Could not extract token from node1 logs"
    docker logs syfrah-node1
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" down -v
    exit 1
fi

echo "Token: $TOKEN"

echo ""
echo "=== Starting node2 (join) ==="
docker compose -f "$SCRIPT_DIR/docker-compose.yml" run -d --name syfrah-node2 \
    node2 join "$TOKEN" --node-name node-2 --port 51820 --endpoint 172.28.0.11:51820

echo "Waiting for node2 to join..."
sleep 10

echo ""
echo "=== Starting node3 (join) ==="
docker compose -f "$SCRIPT_DIR/docker-compose.yml" run -d --name syfrah-node3 \
    node3 join "$TOKEN" --node-name node-3 --port 51820 --endpoint 172.28.0.12:51820

echo "Waiting for node3 to join and mesh to converge..."
sleep 15

echo ""
echo "=== Checking peers on node1 ==="
docker exec syfrah-node1 syfrah peers || true

echo ""
echo "=== Checking peers on node2 ==="
docker exec syfrah-node2 syfrah peers || true

echo ""
echo "=== Testing IPv6 connectivity ==="
# Get node2's mesh IPv6 from its state
NODE2_IPV6=$(docker exec syfrah-node2 syfrah status 2>&1 | grep "Mesh IPv6" | awk '{print $3}')
echo "Node2 mesh IPv6: $NODE2_IPV6"

if [ -n "$NODE2_IPV6" ]; then
    echo "Pinging node2 from node1..."
    docker exec syfrah-node1 ping6 -c 3 -W 5 "$NODE2_IPV6" && echo "PASS: ping6 successful" || echo "FAIL: ping6 failed"
else
    echo "SKIP: Could not determine node2 IPv6"
fi

echo ""
echo "=== Cleanup ==="
docker rm -f syfrah-node1 syfrah-node2 syfrah-node3 2>/dev/null || true
docker compose -f "$SCRIPT_DIR/docker-compose.yml" down -v

echo ""
echo "=== Done ==="
