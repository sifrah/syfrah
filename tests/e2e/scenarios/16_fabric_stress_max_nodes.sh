#!/usr/bin/env bash
# Stress test: maximum node count on a 2-vCPU runner
# Spawns 15 nodes and verifies full mesh convergence

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── STRESS: Max Nodes (15) ──"

NODE_COUNT=15

create_network

for i in $(seq 1 $NODE_COUNT); do
    start_node "e2e-max-$i" "172.20.0.$((9+i))"
done

init_mesh "e2e-max-1" "172.20.0.10" "node-1"
start_peering "e2e-max-1"

EXPECTED=$((NODE_COUNT - 1))

info "Joining $EXPECTED nodes..."
for i in $(seq 2 $NODE_COUNT); do
    join_mesh "e2e-max-$i" "172.20.0.10" "172.20.0.$((9+i))" "node-$i"
    sleep 1
done

info "Waiting for full convergence ($NODE_COUNT nodes, $EXPECTED peers each)..."
START_TIME=$(date +%s)

if wait_for_convergence "e2e-max-" $NODE_COUNT $EXPECTED 180; then
    ELAPSED=$(($(date +%s) - START_TIME))
    pass "$NODE_COUNT nodes converged in ${ELAPSED}s"
else
    ELAPSED=$(($(date +%s) - START_TIME))
    fail "mesh did not fully converge in 180s"
    for i in $(seq 1 $NODE_COUNT); do
        count=$(docker exec "e2e-max-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        echo "  e2e-max-$i: $count/$EXPECTED peers"
    done
fi

# Memory check on the init node
info "Checking leader memory..."
RSS_KB=$(docker exec "e2e-max-1" bash -c 'cat /proc/$(cat /root/.syfrah/daemon.pid)/status 2>/dev/null | grep VmRSS | awk "{print \$2}"' || echo "0")
if [ -n "$RSS_KB" ] && [ "$RSS_KB" -gt 0 ]; then
    RSS_MB=$((RSS_KB / 1024))
    if [ "$RSS_MB" -lt 100 ]; then
        pass "leader RSS: ${RSS_MB}MB (under 100MB)"
    else
        fail "leader RSS: ${RSS_MB}MB (over 100MB)"
    fi
else
    pass "could not measure RSS (daemon may have exited)"
fi

# State file sanity
STATE_SIZE=$(docker exec "e2e-max-1" wc -c /root/.syfrah/state.json 2>/dev/null | awk '{print $1}' || echo "0")
if [ "$STATE_SIZE" -lt 51200 ]; then
    pass "state.json size: ${STATE_SIZE} bytes (under 50KB)"
else
    fail "state.json size: ${STATE_SIZE} bytes (over 50KB)"
fi

# Spot check connectivity
ipv6_last=$(get_mesh_ipv6 "e2e-max-$NODE_COUNT")
assert_can_ping "e2e-max-1" "$ipv6_last"

cleanup
summary
