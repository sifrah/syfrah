#!/usr/bin/env bash
set -euo pipefail

# E2E test: spawn 3 Docker containers and verify they form a WireGuard mesh
# using the syfrah CLI.
#
# Usage: ./tests/e2e/run.sh
#
# Prerequisites: Docker, cargo (builds syfrah first)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
NETWORK="syfrah-e2e"
IMAGE="syfrah-e2e-test"
NODE1="syfrah-e2e-node1"
NODE2="syfrah-e2e-node2"
NODE3="syfrah-e2e-node3"
PIN="4829"
MESH_NAME="e2e-test"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}✓ $1${NC}"; }
fail() { echo -e "${RED}✗ $1${NC}"; FAILED=1; }
info() { echo -e "${YELLOW}→ $1${NC}"; }

FAILED=0

# ── Cleanup function ─────────────────────────────────────────

cleanup() {
    info "Cleaning up..."
    docker rm -f "$NODE1" "$NODE2" "$NODE3" 2>/dev/null || true
    docker network rm "$NETWORK" 2>/dev/null || true
}

trap cleanup EXIT

# ── Build ─────────────────────────────────────────────────────

info "Building Docker image (compiles syfrah inside the container)..."
cd "$REPO_ROOT"
docker build -t "$IMAGE" -f tests/e2e/Dockerfile . --quiet

# ── Setup network ─────────────────────────────────────────────

info "Creating Docker network..."
docker network create "$NETWORK" \
    --subnet 172.20.0.0/24 \
    --driver bridge \
    >/dev/null

# ── Start containers ──────────────────────────────────────────

info "Starting node-1 (init node)..."
docker run -d \
    --name "$NODE1" \
    --network "$NETWORK" \
    --ip 172.20.0.10 \
    --cap-add NET_ADMIN \
    --cap-add NET_RAW \
    --hostname node-1 \
    "$IMAGE" >/dev/null

info "Starting node-2..."
docker run -d \
    --name "$NODE2" \
    --network "$NETWORK" \
    --ip 172.20.0.11 \
    --cap-add NET_ADMIN \
    --cap-add NET_RAW \
    --hostname node-2 \
    "$IMAGE" >/dev/null

info "Starting node-3..."
docker run -d \
    --name "$NODE3" \
    --network "$NETWORK" \
    --ip 172.20.0.12 \
    --cap-add NET_ADMIN \
    --cap-add NET_RAW \
    --hostname node-3 \
    "$IMAGE" >/dev/null

# ── Init mesh on node-1 ──────────────────────────────────────

info "Initializing mesh on node-1..."
docker exec "$NODE1" \
    syfrah fabric init \
    --name "$MESH_NAME" \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    -d

# Wait for daemon to start
sleep 2

info "Starting peering on node-1 with PIN $PIN..."
docker exec "$NODE1" \
    syfrah fabric peering start --pin "$PIN"

# ── Join from node-2 and node-3 ──────────────────────────────

info "Node-2 joining mesh..."
docker exec "$NODE2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-2 \
    --endpoint 172.20.0.11:51820 \
    --pin "$PIN" \
    -d

sleep 2

info "Node-3 joining mesh..."
docker exec "$NODE3" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-3 \
    --endpoint 172.20.0.12:51820 \
    --pin "$PIN" \
    -d

# Wait for mesh to converge
info "Waiting for mesh convergence..."
sleep 5

# ── Verify ────────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════"
echo "  Verification"
echo "═══════════════════════════════════════"
echo ""

# Check node-1 status
info "Checking node-1 status..."
if docker exec "$NODE1" syfrah fabric status 2>&1 | grep -q "running"; then
    pass "node-1 daemon is running"
else
    fail "node-1 daemon is not running"
    docker exec "$NODE1" syfrah fabric status 2>&1 || true
fi

# Check node-2 status
if docker exec "$NODE2" syfrah fabric status 2>&1 | grep -q "running"; then
    pass "node-2 daemon is running"
else
    fail "node-2 daemon is not running"
fi

# Check node-3 status
if docker exec "$NODE3" syfrah fabric status 2>&1 | grep -q "running"; then
    pass "node-3 daemon is running"
else
    fail "node-3 daemon is not running"
fi

# Check peer counts
node1_peers=$(docker exec "$NODE1" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
node2_peers=$(docker exec "$NODE2" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
node3_peers=$(docker exec "$NODE3" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")

if [ "$node1_peers" -eq 2 ]; then
    pass "node-1 sees 2 peers"
else
    fail "node-1 sees $node1_peers peers (expected 2)"
    docker exec "$NODE1" syfrah fabric peers 2>&1 || true
fi

if [ "$node2_peers" -eq 2 ]; then
    pass "node-2 sees 2 peers"
else
    fail "node-2 sees $node2_peers peers (expected 2)"
    docker exec "$NODE2" syfrah fabric peers 2>&1 || true
fi

if [ "$node3_peers" -eq 2 ]; then
    pass "node-3 sees 2 peers"
else
    fail "node-3 sees $node3_peers peers (expected 2)"
    docker exec "$NODE3" syfrah fabric peers 2>&1 || true
fi

# Check WireGuard interfaces exist
if docker exec "$NODE1" ip link show syfrah0 2>&1 | grep -q "syfrah0"; then
    pass "node-1 has syfrah0 interface"
else
    fail "node-1 missing syfrah0 interface"
fi

if docker exec "$NODE2" ip link show syfrah0 2>&1 | grep -q "syfrah0"; then
    pass "node-2 has syfrah0 interface"
else
    fail "node-2 missing syfrah0 interface"
fi

if docker exec "$NODE3" ip link show syfrah0 2>&1 | grep -q "syfrah0"; then
    pass "node-3 has syfrah0 interface"
else
    fail "node-3 missing syfrah0 interface"
fi

# Check mesh IPv6 connectivity (ping via WireGuard tunnel)
node2_ipv6=$(docker exec "$NODE2" syfrah fabric status 2>&1 | grep "Mesh IPv6" | awk '{print $NF}')
node3_ipv6=$(docker exec "$NODE3" syfrah fabric status 2>&1 | grep "Mesh IPv6" | awk '{print $NF}')

if [ -n "$node2_ipv6" ] && [ -n "$node3_ipv6" ]; then
    if docker exec "$NODE1" ping -6 -c 1 -W 3 "$node2_ipv6" >/dev/null 2>&1; then
        pass "node-1 can ping node-2 via mesh ($node2_ipv6)"
    else
        fail "node-1 cannot ping node-2 via mesh ($node2_ipv6)"
    fi

    if docker exec "$NODE1" ping -6 -c 1 -W 3 "$node3_ipv6" >/dev/null 2>&1; then
        pass "node-1 can ping node-3 via mesh ($node3_ipv6)"
    else
        fail "node-1 cannot ping node-3 via mesh ($node3_ipv6)"
    fi

    if docker exec "$NODE2" ping -6 -c 1 -W 3 "$node3_ipv6" >/dev/null 2>&1; then
        pass "node-2 can ping node-3 via mesh ($node3_ipv6)"
    else
        fail "node-2 cannot ping node-3 via mesh ($node3_ipv6)"
    fi
else
    fail "could not extract mesh IPv6 addresses"
fi

# ── Summary ───────────────────────────────────────────────────

echo ""
echo "═══════════════════════════════════════"

if [ "$FAILED" -eq 0 ]; then
    echo -e "  ${GREEN}All tests passed${NC}"
    echo "═══════════════════════════════════════"
    exit 0
else
    echo -e "  ${RED}Some tests failed${NC}"
    echo "═══════════════════════════════════════"
    echo ""
    info "Debug: node-1 peers"
    docker exec "$NODE1" syfrah fabric peers 2>&1 || true
    echo ""
    info "Debug: node-1 status"
    docker exec "$NODE1" syfrah fabric status 2>&1 || true
    exit 1
fi
