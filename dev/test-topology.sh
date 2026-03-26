#!/usr/bin/env bash
#
# Topology test: spin up 4 containers across 2 regions and 2 zones, build the
# mesh, then verify that `syfrah fabric topology` and `syfrah fabric peers
# --topology` report the correct tree.
#
# Prerequisites:
#   - Docker with Compose v2
#   - WireGuard kernel module loaded on the host
#   - syfrah binary at target/debug/syfrah (run `cargo build` first)
#
# Usage:
#   ./dev/test-topology.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/docker-compose.yml"

# Container names
N1="syfrah-node1"
N2="syfrah-node2"
N3="syfrah-node3"
N4="syfrah-node4"

# Helpers
n1() { docker exec "$N1" "$@"; }
n2() { docker exec "$N2" "$@"; }
n3() { docker exec "$N3" "$@"; }
n4() { docker exec "$N4" "$@"; }

die() { echo "FAIL: $1" >&2; cleanup; exit 1; }

pass() { echo "  PASS: $1"; }

cleanup() {
    echo ""
    echo "==> Cleaning up containers..."
    docker compose -f "$COMPOSE_FILE" down --timeout 3 >/dev/null 2>&1 || true
}
trap cleanup EXIT

# ─── Pre-checks ───────────────────────────────────────────────────────────────

echo "==> Checking prerequisites..."

if ! lsmod | grep -q wireguard 2>/dev/null; then
    echo "    Loading wireguard kernel module..."
    sudo modprobe wireguard || die "Could not load wireguard module. Install: sudo apt install wireguard"
fi

BINARY="$SCRIPT_DIR/../target/debug/syfrah"
if [ ! -f "$BINARY" ]; then
    die "Binary not found at $BINARY. Run: cargo build"
fi

# ─── Start 4 containers ──────────────────────────────────────────────────────

echo "==> Starting 4 containers (2 regions, 2 zones)..."
docker compose -f "$COMPOSE_FILE" down --timeout 2 >/dev/null 2>&1 || true
docker compose -f "$COMPOSE_FILE" up -d --build >/dev/null 2>&1
sleep 1

# Verify all four are running
for c in "$N1" "$N2" "$N3" "$N4"; do
    docker inspect "$c" --format '{{.State.Running}}' | grep -q true \
        || die "$c is not running"
done
echo "    All 4 containers running."

# ─── Get bridge IPs ──────────────────────────────────────────────────────────

NODE1_IP=$(docker inspect "$N1" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
NODE2_IP=$(docker inspect "$N2" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
NODE3_IP=$(docker inspect "$N3" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')
NODE4_IP=$(docker inspect "$N4" --format '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}')

echo "    node1=$NODE1_IP  node2=$NODE2_IP  node3=$NODE3_IP  node4=$NODE4_IP"

# ─── Init mesh on node1 (eu-west / par-1) ────────────────────────────────────

echo ""
echo "==> Init mesh on node1 (region=eu-west, zone=par-1)..."
INIT_OUTPUT=$(n1 syfrah fabric init \
    --name topology-test \
    --node-name node1 \
    --endpoint "${NODE1_IP}:51820" \
    --region eu-west \
    --zone par-1 \
    --peering 2>&1)
echo "$INIT_OUTPUT"

PIN=$(echo "$INIT_OUTPUT" | grep '^ *PIN:' | awk '{print $NF}')
[ -n "$PIN" ] || die "Could not extract PIN from init output"
echo "    PIN: $PIN"

# ─── Join node2 (eu-west / par-1) ────────────────────────────────────────────

echo ""
echo "==> Join node2 (region=eu-west, zone=par-1)..."
n2 syfrah fabric join "$NODE1_IP" \
    --node-name node2 \
    --endpoint "${NODE2_IP}:51820" \
    --region eu-west \
    --zone par-1 \
    --pin "$PIN" 2>&1
echo "    Joined."

# ─── Join node3 (us-east / use-1) ────────────────────────────────────────────

echo ""
echo "==> Join node3 (region=us-east, zone=use-1)..."
n3 syfrah fabric join "$NODE1_IP" \
    --node-name node3 \
    --endpoint "${NODE3_IP}:51820" \
    --region us-east \
    --zone use-1 \
    --pin "$PIN" 2>&1
echo "    Joined."

# ─── Join node4 (us-east / use-1) ────────────────────────────────────────────

echo ""
echo "==> Join node4 (region=us-east, zone=use-1)..."
n4 syfrah fabric join "$NODE1_IP" \
    --node-name node4 \
    --endpoint "${NODE4_IP}:51820" \
    --region us-east \
    --zone use-1 \
    --pin "$PIN" 2>&1
echo "    Joined."

# ─── Wait for peering to propagate ───────────────────────────────────────────

echo ""
echo "==> Waiting for mesh convergence..."
sleep 4

# ─── Verify: syfrah fabric topology ──────────────────────────────────────────

echo ""
echo "==> TEST 1: syfrah fabric topology"
TOPO_OUTPUT=$(n1 syfrah fabric topology 2>&1)
echo "$TOPO_OUTPUT"

# Check summary line: 2 regions, 2 zones, 4 nodes
if echo "$TOPO_OUTPUT" | grep -qE "2 regions.*2 zones.*4 nodes"; then
    pass "Topology summary shows 2 regions, 2 zones, 4 nodes"
else
    die "Topology summary mismatch. Expected 2 regions, 2 zones, 4 nodes."
fi

# Check eu-west region is present
if echo "$TOPO_OUTPUT" | grep -q "eu-west"; then
    pass "Region eu-west present"
else
    die "Region eu-west not found in topology output"
fi

# Check us-east region is present
if echo "$TOPO_OUTPUT" | grep -q "us-east"; then
    pass "Region us-east present"
else
    die "Region us-east not found in topology output"
fi

# Check par-1 zone is present
if echo "$TOPO_OUTPUT" | grep -q "par-1"; then
    pass "Zone par-1 present"
else
    die "Zone par-1 not found in topology output"
fi

# Check use-1 zone is present
if echo "$TOPO_OUTPUT" | grep -q "use-1"; then
    pass "Zone use-1 present"
else
    die "Zone use-1 not found in topology output"
fi

# ─── Verify: syfrah fabric topology --json ───────────────────────────────────

echo ""
echo "==> TEST 2: syfrah fabric topology --json"
TOPO_JSON=$(n1 syfrah fabric topology --json 2>&1)
echo "$TOPO_JSON"

REGION_COUNT=$(echo "$TOPO_JSON" | jq '.regions | length')
ZONE_COUNT=$(echo "$TOPO_JSON" | jq '[.regions[].zones[]] | length')
NODE_COUNT=$(echo "$TOPO_JSON" | jq '.total_nodes')

if [ "$REGION_COUNT" -eq 2 ]; then
    pass "JSON: 2 regions"
else
    die "JSON: expected 2 regions, got $REGION_COUNT"
fi

if [ "$ZONE_COUNT" -eq 2 ]; then
    pass "JSON: 2 zones"
else
    die "JSON: expected 2 zones, got $ZONE_COUNT"
fi

if [ "$NODE_COUNT" -eq 4 ]; then
    pass "JSON: 4 nodes"
else
    die "JSON: expected 4 nodes, got $NODE_COUNT"
fi

# ─── Verify: syfrah fabric peers --topology ──────────────────────────────────

echo ""
echo "==> TEST 3: syfrah fabric peers --topology"
PEERS_TOPO=$(n1 syfrah fabric peers --topology 2>&1)
echo "$PEERS_TOPO"

# Peers --topology groups by region then zone. Verify both regions appear.
if echo "$PEERS_TOPO" | grep -q "eu-west" && echo "$PEERS_TOPO" | grep -q "us-east"; then
    pass "peers --topology groups both regions"
else
    die "peers --topology missing region grouping"
fi

# Verify all 4 node names appear (node1 is self, but peers shows the other 3 + self in topology view)
for name in node1 node2 node3 node4; do
    if echo "$PEERS_TOPO" | grep -q "$name"; then
        pass "peers --topology includes $name"
    else
        die "peers --topology missing $name"
    fi
done

# ─── Summary ─────────────────────────────────────────────────────────────────

echo ""
echo "========================================"
echo "  ALL TOPOLOGY TESTS PASSED"
echo "  4 containers, 2 regions, 2 zones"
echo "  eu-west/par-1: node1, node2"
echo "  us-east/use-1: node3, node4"
echo "========================================"
