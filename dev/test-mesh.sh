#!/usr/bin/env bash
#
# Peers node1 and node2 via syfrah fabric, then tests mesh ping in both directions.
# Expects containers to be running (./dev.sh up).
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

N1="syfrah-node1"
N2="syfrah-node2"

n1() { docker exec "$N1" "$@"; }
n2() { docker exec "$N2" "$@"; }

cleanup() {
    echo ""
    echo "==> Cleaning up (restarting containers for fresh state)..."
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" restart --timeout 2 >/dev/null 2>&1
    echo "    Done."
}

die() { echo "FAIL: $1" >&2; exit 1; }

# ---- Pre-checks ----
echo "==> Checking containers are running..."
docker inspect "$N1" --format '{{.State.Running}}' | grep -q true || die "$N1 is not running. Run: ./dev.sh up"
docker inspect "$N2" --format '{{.State.Running}}' | grep -q true || die "$N2 is not running. Run: ./dev.sh up"

# ---- Clean state (recreate containers for a fresh environment) ----
echo "==> Recreating containers for clean state..."
docker compose -f "$SCRIPT_DIR/docker-compose.yml" down --timeout 2 >/dev/null 2>&1
docker compose -f "$SCRIPT_DIR/docker-compose.yml" up -d >/dev/null 2>&1
sleep 1

# ---- Get node IPs on docker bridge ----
NODE1_IP=$(docker inspect "$N1" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
NODE2_IP=$(docker inspect "$N2" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
echo "    node1 bridge IP: $NODE1_IP"
echo "    node2 bridge IP: $NODE2_IP"

# ---- Init mesh on node1 ----
echo "==> Initializing mesh on node1..."
INIT_OUTPUT=$(n1 syfrah fabric init \
    --name testmesh \
    --node-name node1 \
    --endpoint "${NODE1_IP}:51820" \
    --peering 2>&1)
echo "$INIT_OUTPUT"

# Extract PIN from output
PIN=$(echo "$INIT_OUTPUT" | grep '^ *PIN:' | awk '{print $NF}')
[ -n "$PIN" ] || die "Could not extract PIN from init output"
echo "    Extracted PIN: $PIN"

# ---- Join from node2 ----
echo ""
echo "==> Joining mesh from node2..."
JOIN_OUTPUT=$(n2 syfrah fabric join "$NODE1_IP" \
    --node-name node2 \
    --endpoint "${NODE2_IP}:51820" \
    --pin "$PIN" 2>&1)
echo "$JOIN_OUTPUT"

# ---- Wait for WireGuard handshake ----
echo ""
echo "==> Waiting for handshake..."
sleep 2

# ---- Get mesh IPv6 addresses ----
NODE1_MESH_IP=$(n1 syfrah fabric status 2>&1 | grep 'Mesh IPv6:' | awk '{print $NF}')
NODE2_MESH_IP=$(n2 syfrah fabric status 2>&1 | grep 'Mesh IPv6:' | awk '{print $NF}')

[ -n "$NODE1_MESH_IP" ] || die "Could not get node1 mesh IPv6"
[ -n "$NODE2_MESH_IP" ] || die "Could not get node2 mesh IPv6"

echo "    node1 mesh: $NODE1_MESH_IP"
echo "    node2 mesh: $NODE2_MESH_IP"

# ---- Ping tests ----
echo ""
echo "==> Ping: node1 -> node2 (mesh)"
if n1 ping6 -c 15 -i 1 -W 5 "$NODE2_MESH_IP"; then
    echo "    PASS"
else
    die "node1 -> node2 ping failed"
fi

echo ""
echo "==> Ping: node2 -> node1 (mesh)"
if n2 ping6 -c 15 -i 1 -W 5 "$NODE1_MESH_IP"; then
    echo "    PASS"
else
    die "node2 -> node1 ping failed"
fi

# ---- Summary ----
echo ""
echo "========================================"
echo "  ALL TESTS PASSED"
echo "  node1 ($NODE1_MESH_IP) <-> node2 ($NODE2_MESH_IP)"
echo "========================================"
