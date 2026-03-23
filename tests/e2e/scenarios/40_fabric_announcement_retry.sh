#!/usr/bin/env bash
# Scenario: Announcement retry after temporary network failure

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Announcement Retry ──"
create_network

start_node "e2e-retry-1" "${E2E_IP_PREFIX}.10"
start_node "e2e-retry-2" "${E2E_IP_PREFIX}.11"
start_node "e2e-retry-3" "${E2E_IP_PREFIX}.12"

# Form 2-node mesh
init_mesh "e2e-retry-1" "${E2E_IP_PREFIX}.10" "node-1"
start_peering "e2e-retry-1"
join_mesh "e2e-retry-2" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.11" "node-2"
if ! wait_for_convergence "e2e-retry-" 2 1 30; then
    fail "initial 2-node mesh did not converge"
    cleanup; summary
fi

# Block traffic from node-1 to node-2 (announcements will fail initially)
info "Blocking traffic node-1 -> node-2"
block_traffic "e2e-retry-1" "${E2E_IP_PREFIX}.11"

# Join node-3 while node-2 is unreachable from node-1
info "Joining node-3 while node-2 is blocked..."
join_mesh "e2e-retry-3" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.12" "node-3"
sleep 2

# Unblock traffic — retry should eventually succeed
info "Unblocking traffic"
unblock_traffic "e2e-retry-1" "${E2E_IP_PREFIX}.11"

# Node-2 should learn about node-3 via retry or reconciliation
info "Waiting for node-2 to discover node-3..."
if wait_for_convergence "e2e-retry-" 3 2 60; then
    pass "all nodes converged after announcement retry"
else
    fail "convergence timed out"
    for i in 1 2 3; do
        count=$(docker exec "e2e-retry-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        debug "e2e-retry-$i sees $count peers"
    done
fi

# Verify bidirectional connectivity
ipv6_3=$(get_mesh_ipv6 "e2e-retry-3")
if [ -n "$ipv6_3" ]; then
    assert_can_ping "e2e-retry-2" "$ipv6_3"
fi

cleanup
summary
