#!/usr/bin/env bash
# Scenario: UX Flow — Add 5 nodes
# Validates scaling works, data propagates correctly.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: Scaling ──"
trap cleanup EXIT
create_network

NODE_COUNT=5

# Start all nodes
for i in $(seq 1 $NODE_COUNT); do
    start_node "e2e-flow-scale-${i}" "172.20.0.$((9 + i))"
done

# Server 1: init + peering
info "Server 1: init and start peering..."
init_mesh "e2e-flow-scale-1" "172.20.0.10" "scale-node-1"
start_peering "e2e-flow-scale-1"

# Servers 2-5: join sequentially
for i in $(seq 2 $NODE_COUNT); do
    info "Server $i: joining mesh..."
    join_mesh "e2e-flow-scale-${i}" "172.20.0.10" "172.20.0.$((9 + i))" "scale-node-${i}"
    sleep 3
done

# Wait for convergence
info "Waiting for full convergence..."
EXPECTED_PEERS=$((NODE_COUNT - 1))
if wait_for_convergence "e2e-flow-scale-" "$NODE_COUNT" "$EXPECTED_PEERS" 90; then
    pass "all $NODE_COUNT nodes converged to $EXPECTED_PEERS peers"
else
    fail "convergence timeout"
    for i in $(seq 1 $NODE_COUNT); do
        info "e2e-flow-scale-${i} peers:"
        docker exec "e2e-flow-scale-${i}" syfrah fabric peers 2>&1 || true
    done
fi

# Validate each node
for i in $(seq 1 $NODE_COUNT); do
    node="e2e-flow-scale-${i}"
    info "Validating $node..."

    # No duplicates
    assert_no_duplicate_peers "$node"

    # Region/zone displayed
    assert_regions_displayed "$node"

    # No epoch dates
    assert_no_epoch_dates "$node"

    # Correct peer count
    assert_peer_count "$node" "$EXPECTED_PEERS"
done

cleanup
summary
