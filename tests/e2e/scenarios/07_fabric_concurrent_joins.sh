#!/usr/bin/env bash
# Scenario: Multiple nodes join simultaneously (race condition test)
# Verifies no duplicate peers or lost state under concurrent joins

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Concurrent Joins ──"

create_network

start_node "e2e-conc-1" "172.20.0.10"
start_node "e2e-conc-2" "172.20.0.11"
start_node "e2e-conc-3" "172.20.0.12"
start_node "e2e-conc-4" "172.20.0.13"
start_node "e2e-conc-5" "172.20.0.14"

init_mesh "e2e-conc-1" "172.20.0.10" "node-1"
start_peering "e2e-conc-1"

# Join 4 nodes with zero delay — maximum concurrency pressure
info "Joining 4 nodes simultaneously..."
for i in 2 3 4 5; do
    docker exec -d "e2e-conc-$i" \
        syfrah fabric join 172.20.0.10:51821 \
        --node-name "node-$i" \
        --endpoint "172.20.0.$((9+i)):51820" \
        --pin "$E2E_PIN"
done

# Wait for all daemons
for i in 2 3 4 5; do
    wait_daemon "e2e-conc-$i" 30 || true
done

# All nodes must converge — race condition is fixed
info "Waiting for convergence..."
if wait_for_convergence "e2e-conc-" 5 4 90; then
    pass "all 5 nodes converged to 4 peers"
else
    fail "convergence timed out"
    for i in 1 2 3 4 5; do
        count=$(docker exec "e2e-conc-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        debug "e2e-conc-$i sees $count peers"
    done
fi

# Verify no duplicate WG keys in leader state
info "Checking for duplicate peers..."
dupes=$(docker exec "e2e-conc-1" cat /root/.syfrah/state.json 2>/dev/null | \
    jq '[.peers[].wg_public_key] | length - ([.peers[].wg_public_key] | unique | length)' 2>/dev/null || echo "0")
if [ "$dupes" = "0" ]; then
    pass "no duplicate peers in state"
else
    fail "$dupes duplicate peer(s) found in state"
fi

cleanup
summary
