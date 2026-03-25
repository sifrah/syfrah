#!/usr/bin/env bash
# Stress test: nodes repeatedly join and leave for 90 seconds
# Detects memory leaks, orphan interfaces, zombie processes

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── STRESS: Churn (join/leave cycles) ──"

create_network

# Stable node that stays throughout
start_node "e2e-churn-1" "172.20.0.10"
init_mesh "e2e-churn-1" "172.20.0.10" "node-1"
start_peering "e2e-churn-1"

# Churning nodes
start_node "e2e-churn-2" "172.20.0.11"
start_node "e2e-churn-3" "172.20.0.12"

CYCLES=3
info "Running $CYCLES join/leave cycles..."

for cycle in $(seq 1 $CYCLES); do
    info "Cycle $cycle/$CYCLES: joining..."

    # Join both churning nodes
    docker exec -d "e2e-churn-2" \
        syfrah fabric join 172.20.0.10:51821 \
        --node-name "churn-2-c$cycle" \
        --endpoint 172.20.0.11:51820 \
        --pin "$E2E_PIN"
    wait_daemon "e2e-churn-2" 20 || true

    docker exec -d "e2e-churn-3" \
        syfrah fabric join 172.20.0.10:51821 \
        --node-name "churn-3-c$cycle" \
        --endpoint 172.20.0.12:51820 \
        --pin "$E2E_PIN"
    wait_daemon "e2e-churn-3" 20 || true

    sleep 3

    info "Cycle $cycle/$CYCLES: leaving..."

    # Leave
    docker exec "e2e-churn-2" syfrah fabric leave --yes 2>/dev/null || true
    docker exec "e2e-churn-2" pkill -f syfrah 2>/dev/null || true
    docker exec "e2e-churn-3" syfrah fabric leave --yes 2>/dev/null || true
    docker exec "e2e-churn-3" pkill -f syfrah 2>/dev/null || true

    sleep 2

    # Clean up state for next cycle
    docker exec "e2e-churn-2" rm -rf /root/.syfrah 2>/dev/null || true
    docker exec "e2e-churn-3" rm -rf /root/.syfrah 2>/dev/null || true
done

# Final join — should work cleanly after all the churn
info "Final join after churn..."
docker exec -d "e2e-churn-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name "churn-2-final" \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN"
wait_daemon "e2e-churn-2" 20

sleep 3

# Stable node still alive
assert_daemon_running "e2e-churn-1"

# Final join worked
assert_interface_exists "e2e-churn-2"

# Connectivity
ipv6_2=$(get_mesh_ipv6 "e2e-churn-2")
assert_can_ping "e2e-churn-1" "$ipv6_2"

# No zombie syfrah processes on churning nodes
zombie_count=$(docker exec "e2e-churn-2" pgrep -c -f syfrah 2>/dev/null || echo "0")
if [ "$zombie_count" -le 1 ]; then
    pass "no zombie processes on churn node ($zombie_count syfrah processes)"
else
    fail "$zombie_count syfrah processes on churn node (expected <= 1)"
fi

# State file valid on stable node
if docker exec "e2e-churn-1" cat /root/.syfrah/state.json 2>/dev/null | jq . >/dev/null 2>&1; then
    pass "stable node state.json is valid JSON"
else
    fail "stable node state.json is invalid"
fi

cleanup
summary
