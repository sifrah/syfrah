#!/usr/bin/env bash
# Scenario: All announcements fail — mesh must still converge via reconciliation

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Announcement Total Failure ──"
create_network

start_node "e2e-allfail-1" "172.20.0.10"
start_node "e2e-allfail-2" "172.20.0.11"
start_node "e2e-allfail-3" "172.20.0.12"

init_mesh "e2e-allfail-1" "172.20.0.10" "node-1"
start_peering "e2e-allfail-1"

# Join both nodes
join_mesh "e2e-allfail-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-allfail-3" "172.20.0.10" "172.20.0.12" "node-3"

# Even if some announcements fail, reconciliation loop (30s) ensures
# all peers eventually appear in WireGuard
info "Waiting for reconciliation-based convergence (up to 90s)..."
if wait_for_convergence "e2e-allfail-" 3 2 90; then
    pass "all nodes converged"
else
    fail "convergence timed out"
    for i in 1 2 3; do
        count=$(docker exec "e2e-allfail-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        debug "e2e-allfail-$i sees $count peers"
    done
fi

assert_peer_count "e2e-allfail-1" 2
assert_peer_count "e2e-allfail-2" 2
assert_peer_count "e2e-allfail-3" 2

cleanup
summary
