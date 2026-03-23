#!/usr/bin/env bash
# Scenario: 10-node mesh convergence time measurement

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Convergence Time (10 nodes) ──"

create_network

for i in $(seq 1 10); do
    start_node "e2e-conv-$i" "${E2E_IP_PREFIX}.$((9+i))"
done

init_mesh "e2e-conv-1" "${E2E_IP_PREFIX}.10" "node-1"
start_peering "e2e-conv-1"

START_TIME=$(date +%s)

for i in $(seq 2 10); do
    join_mesh "e2e-conv-$i" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.$((9+i))" "node-$i"
    sleep 1
done

# Wait for convergence (all see 9 peers)
info "Waiting for full convergence..."
if wait_for_convergence "e2e-conv-" 10 9 60; then
    ELAPSED=$(($(date +%s) - START_TIME))
    pass "10 nodes converged in ${ELAPSED}s"
    if [ "$ELAPSED" -le 45 ]; then
        pass "convergence time under 45s threshold"
    else
        fail "convergence took ${ELAPSED}s (threshold: 45s)"
    fi
else
    ELAPSED=$(($(date +%s) - START_TIME))
    fail "mesh did not converge within 60s"
    for i in $(seq 1 10); do
        count=$(docker exec "e2e-conv-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        echo "  e2e-conv-$i: $count peers"
    done
fi

# Spot check connectivity
ipv6_10=$(get_mesh_ipv6 "e2e-conv-10")
assert_can_ping "e2e-conv-1" "$ipv6_10"

cleanup
summary
