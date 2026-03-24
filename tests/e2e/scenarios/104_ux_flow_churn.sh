#!/usr/bin/env bash
# Scenario: UX Flow — Nodes coming and going (churn)
# Validates churn doesn't corrupt state.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: Churn ──"
trap cleanup EXIT
create_network

start_node "e2e-flow-churn-1" "172.20.0.10"
start_node "e2e-flow-churn-2" "172.20.0.11"
start_node "e2e-flow-churn-3" "172.20.0.12"

# Setup 3-node mesh
info "Setting up 3-node mesh..."
init_mesh "e2e-flow-churn-1" "172.20.0.10" "churn-srv-1"
start_peering "e2e-flow-churn-1"
join_mesh "e2e-flow-churn-2" "172.20.0.10" "172.20.0.11" "churn-srv-2"
sleep 3
join_mesh "e2e-flow-churn-3" "172.20.0.10" "172.20.0.12" "churn-srv-3"

sleep 5

info "Verifying initial mesh..."
if wait_for_convergence "e2e-flow-churn-" 3 2 60; then
    pass "initial 3-node mesh converged"
else
    fail "initial convergence failed"
fi

# Repeat leave/rejoin 3 times
for round in 1 2 3; do
    info "Churn round $round: server-3 leaves..."
    docker exec "e2e-flow-churn-3" syfrah fabric leave 2>&1 || true
    sleep 3

    info "Churn round $round: server-3 rejoins..."
    start_peering "e2e-flow-churn-1"
    join_mesh "e2e-flow-churn-3" "172.20.0.10" "172.20.0.12" "churn-srv-3"
    sleep 5

    # Quick convergence check
    if wait_for_convergence "e2e-flow-churn-" 3 2 60; then
        pass "round $round: reconverged"
    else
        fail "round $round: convergence failed"
    fi
done

# Final validation: all 3 nodes in good state
info "Final validation..."
for i in 1 2 3; do
    node="e2e-flow-churn-${i}"

    # Each sees 2 peers
    assert_peer_count "$node" 2

    # No duplicates
    assert_no_duplicate_peers "$node"

    # No epoch dates
    assert_no_epoch_dates "$node"
done

cleanup
summary
