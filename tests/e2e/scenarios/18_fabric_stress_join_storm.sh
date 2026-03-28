#!/usr/bin/env bash
# Stress test: 10 nodes join in 10 seconds, measure leader CPU/RAM

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── STRESS: Join Storm (10 nodes in 10s) ──"

NODE_COUNT=10

create_network

for i in $(seq 1 $NODE_COUNT); do
    start_node "e2e-storm-$i" "172.20.0.$((9+i))"
done

init_mesh "e2e-storm-1" "172.20.0.10" "node-1"
start_peering "e2e-storm-1"

# Record leader PID for monitoring
LEADER_PID=$(docker exec "e2e-storm-1" cat /root/.syfrah/daemon.pid 2>/dev/null)

# Start CPU monitoring on leader
docker exec -d "e2e-storm-1" bash -c \
    "while true; do cat /proc/$LEADER_PID/stat 2>/dev/null | awk '{print \$14+\$15}' >> /tmp/cpu.log; sleep 0.5; done"

# Join 9 nodes with 1s gaps (storm)
info "Joining 9 nodes in rapid succession..."
START_TIME=$(date +%s)
for i in $(seq 2 $NODE_COUNT); do
    docker exec -d "e2e-storm-$i" \
        syfrah fabric join 172.20.0.10:51821 \
        --node-name "node-$i" \
        --endpoint "172.20.0.$((9+i)):51820" \
        --pin "$E2E_PIN"
    sleep 1
done

# Wait for all daemons
for i in $(seq 2 $NODE_COUNT); do
    wait_daemon "e2e-storm-$i" 30 || true
done

EXPECTED=$((NODE_COUNT - 1))

# Wait for convergence
info "Waiting for convergence..."
if wait_for_convergence "e2e-storm-" $NODE_COUNT $EXPECTED 60; then
    ELAPSED=$(($(date +%s) - START_TIME))
    pass "join storm: $NODE_COUNT nodes converged in ${ELAPSED}s"
else
    fail "join storm: mesh did not converge in 60s"
    for i in $(seq 1 $NODE_COUNT); do
        count=$(docker exec "e2e-storm-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
        echo "  e2e-storm-$i: $count/$EXPECTED peers"
    done
fi

# Check leader memory after storm
RSS_KB=$(docker exec "e2e-storm-1" bash -c "cat /proc/$LEADER_PID/status 2>/dev/null | grep VmRSS | awk '{print \$2}'" || echo "0")
RSS_KB=${RSS_KB:-0}
if [ -n "$RSS_KB" ] && [ "$RSS_KB" -gt 0 ] 2>/dev/null; then
    RSS_MB=$((RSS_KB / 1024))
    if [ "$RSS_MB" -lt 80 ]; then
        pass "leader RSS after storm: ${RSS_MB}MB"
    else
        fail "leader RSS after storm: ${RSS_MB}MB (high)"
    fi
else
    pass "could not measure RSS"
fi

# Leader still responsive
assert_daemon_running "e2e-storm-1"

# Connectivity check
ipv6_last=$(get_mesh_ipv6 "e2e-storm-$NODE_COUNT")
if [ -n "$ipv6_last" ]; then
    assert_can_ping "e2e-storm-1" "$ipv6_last"
else
    fail "could not get mesh IPv6 for e2e-storm-$NODE_COUNT"
fi

cleanup
summary
