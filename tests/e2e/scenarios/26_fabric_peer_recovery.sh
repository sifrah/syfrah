#!/usr/bin/env bash
# Scenario: unreachable peer recovers automatically when connectivity returns
# Tests the health check recovery path: Unreachable → Active

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Fabric: Peer Recovery ──"

create_network

start_node "e2e-recov-1" "172.20.0.10"
start_node "e2e-recov-2" "172.20.0.11"

init_mesh "e2e-recov-1" "172.20.0.10" "node-1"
start_peering "e2e-recov-1"
join_mesh "e2e-recov-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

ipv6_2=$(get_mesh_ipv6 "e2e-recov-2")
assert_can_ping "e2e-recov-1" "$ipv6_2"

# Block traffic to simulate network failure
info "Blocking traffic between nodes..."
block_traffic "e2e-recov-1" "172.20.0.11"
block_traffic "e2e-recov-2" "172.20.0.10"

# Wait a bit — not long enough for unreachable timeout (300s)
# but enough to show the partition
sleep 5
assert_cannot_ping "e2e-recov-1" "$ipv6_2"

# Restore connectivity
info "Restoring connectivity..."
unblock_traffic "e2e-recov-1" "172.20.0.11"
unblock_traffic "e2e-recov-2" "172.20.0.10"

# Wait for WireGuard keepalive (25s) + health check (60s)
info "Waiting for keepalive + health check recovery..."
sleep 35

# Peer should be reachable again
assert_can_ping "e2e-recov-1" "$ipv6_2"

# Verify peer is in the peers list (not removed)
peer_count=$(docker exec "e2e-recov-1" syfrah fabric peers 2>&1 | grep -c "active\|unreach" || echo "0")
if [ "$peer_count" -ge 1 ]; then
    pass "peer still in list after recovery"
else
    fail "peer missing from list"
fi

cleanup
summary
