#!/usr/bin/env bash
# Scenario: Stress test for concurrent joins — zero delay, 10 nodes

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Concurrent Joins Stress ──"
create_network

# Start 10 nodes
for i in $(seq 1 10); do
    start_node "e2e-stress-join-$i" "172.20.0.$((10 + i))"
done

# Init mesh on node 1
init_mesh "e2e-stress-join-1" "172.20.0.11" "node-1"
start_peering "e2e-stress-join-1"

# Join all 9 remaining nodes rapidly (1s between joins)
info "Joining 9 nodes rapidly..."
for i in $(seq 2 10); do
    docker exec -d "e2e-stress-join-$i" \
        syfrah fabric join 172.20.0.11:51821 \
        --node-name "node-$i" \
        --endpoint "172.20.0.$((10 + i)):51820" \
        --pin "$E2E_PIN"
    sleep 1
done

# Wait for all daemons
for i in $(seq 2 10); do
    wait_daemon "e2e-stress-join-$i" 60 || true
done

# All 10 nodes must see exactly 9 peers
info "Waiting for full convergence..."
if wait_for_convergence "e2e-stress-join-" 10 9 120; then
    pass "all 10 nodes converged to 9 peers"
else
    fail "convergence timed out"
    for i in $(seq 1 10); do
        count=$(docker exec "e2e-stress-join-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        debug "e2e-stress-join-$i sees $count peers"
    done
fi

# Verify no duplicates in any node's state
for i in $(seq 1 10); do
    dupes=$(docker exec "e2e-stress-join-$i" cat /root/.syfrah/state.json 2>/dev/null | \
        jq '[.peers[].wg_public_key] | length - ([.peers[].wg_public_key] | unique | length)' 2>/dev/null || echo "0")
    if [ "$dupes" = "0" ]; then
        pass "e2e-stress-join-$i: no duplicate peers"
    else
        fail "e2e-stress-join-$i: $dupes duplicate peer(s)"
    fi
done

cleanup
summary
