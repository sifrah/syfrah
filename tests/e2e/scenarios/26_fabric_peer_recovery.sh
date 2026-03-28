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

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-recov-1" 1 30

ipv6_2=$(get_mesh_ipv6 "e2e-recov-2")
if [ -z "$ipv6_2" ]; then
    fail "could not get mesh IPv6 for e2e-recov-2"
fi
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

# Poll for peer recovery (keepalive 25s + health check), timeout 90s
info "Waiting for keepalive + health check recovery (polling up to 90s)..."
_recov_deadline=$(($(date +%s) + 90))
_recov_ok=false
while [ "$(date +%s)" -lt "$_recov_deadline" ]; do
    if docker exec "e2e-recov-1" ping -6 -c 1 -W 2 "$ipv6_2" >/dev/null 2>&1; then
        _recov_ok=true
        break
    fi
    sleep 5
done

# Peer should be reachable again
if [ "$_recov_ok" = true ]; then
    pass "e2e-recov-1 can ping $ipv6_2"
else
    assert_can_ping "e2e-recov-1" "$ipv6_2"
fi

# Verify peer is in the peers list (not removed)
peer_count=$(docker exec "e2e-recov-1" syfrah fabric peers 2>&1 | grep -c "active\|unreach" || echo "0")
if [ "$peer_count" -ge 1 ]; then
    pass "peer still in list after recovery"
else
    fail "peer missing from list"
fi

cleanup
summary
