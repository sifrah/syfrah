#!/usr/bin/env bash
# Scenario: mix of auto and manual zones in the same mesh

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Mixed Auto + Manual ──"

create_network

start_node "e2e-zmix-1" "172.20.0.10"
start_node "e2e-zmix-2" "172.20.0.11"
start_node "e2e-zmix-3" "172.20.0.12"

# Node-1: default region/zone
init_mesh "e2e-zmix-1" "172.20.0.10" "node-1"
start_peering "e2e-zmix-1"

# Node-2: manual region/zone
docker exec -d "e2e-zmix-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-2 \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN" \
    --region custom-dc \
    --zone custom-dc-rack1

wait_daemon "e2e-zmix-2"

# Node-3: default (auto-increment)
join_mesh "e2e-zmix-3" "172.20.0.10" "172.20.0.12" "node-3"

wait_for_convergence "e2e-zmix-" 3 2 30 || true

# Verify all 3 nodes connected
assert_peer_count "e2e-zmix-1" 2

# Verify node-2 has custom region
r2=$(docker exec "e2e-zmix-2" syfrah fabric status 2>&1 | grep "Region:" | awk '{print $2}')
if [ "$r2" = "custom-dc" ]; then
    pass "node-2 custom region preserved in mesh"
else
    fail "node-2 region: $r2 (expected custom-dc)"
fi

# Verify connectivity works regardless of different regions
ipv6_2=$(get_mesh_ipv6 "e2e-zmix-2")
assert_can_ping "e2e-zmix-1" "$ipv6_2"

cleanup
summary
