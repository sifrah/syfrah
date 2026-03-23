#!/usr/bin/env bash
# Stress test: 12-node mesh, verify O(N²) announcements converge
# Each node must see exactly 11 peers after all announcements propagate

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── STRESS: Announcement Flood (12 nodes) ──"

NODE_COUNT=12
EXPECTED=$((NODE_COUNT - 1))

create_network

for i in $(seq 1 $NODE_COUNT); do
    start_node "e2e-flood-$i" "172.20.0.$((9+i))"
done

init_mesh "e2e-flood-1" "172.20.0.10" "node-1"
start_peering "e2e-flood-1"

info "Joining $EXPECTED nodes sequentially..."
START_TIME=$(date +%s)
for i in $(seq 2 $NODE_COUNT); do
    join_mesh "e2e-flood-$i" "172.20.0.10" "172.20.0.$((9+i))" "node-$i"
    sleep 1
done
JOIN_TIME=$(($(date +%s) - START_TIME))
info "All joins completed in ${JOIN_TIME}s"

# Now wait for the O(N²) announcement storm to settle
# Each new join triggers N-1 announcements to existing peers
# Total announcements: sum(1..N-1) = N*(N-1)/2 = 66 TCP connections
info "Waiting for announcement flood to settle (N*(N-1)/2 = $((NODE_COUNT * EXPECTED / 2)) announcements)..."

CONVERGE_START=$(date +%s)
if wait_for_convergence "e2e-flood-" $NODE_COUNT $EXPECTED 90; then
    CONVERGE_TIME=$(($(date +%s) - CONVERGE_START))
    pass "all $NODE_COUNT nodes converged in ${CONVERGE_TIME}s after joins"
else
    CONVERGE_TIME=$(($(date +%s) - CONVERGE_START))
    fail "did not converge in 90s"
fi

# Report per-node peer count
info "Per-node peer counts:"
ALL_OK=true
for i in $(seq 1 $NODE_COUNT); do
    count=$(docker exec "e2e-flood-$i" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
    if [ "$count" -eq $EXPECTED ]; then
        echo "  e2e-flood-$i: $count/$EXPECTED ✓"
    else
        echo "  e2e-flood-$i: $count/$EXPECTED ✗"
        ALL_OK=false
    fi
done

if [ "$ALL_OK" = true ]; then
    pass "all nodes see $EXPECTED peers"
else
    fail "some nodes have incomplete peer views"
fi

# Full connectivity spot check (first <-> last)
ipv6_first=$(get_mesh_ipv6 "e2e-flood-1")
ipv6_last=$(get_mesh_ipv6 "e2e-flood-$NODE_COUNT")
assert_can_ping "e2e-flood-1" "$ipv6_last"
assert_can_ping "e2e-flood-$NODE_COUNT" "$ipv6_first"

# Mid-mesh connectivity (node-6 <-> node-7)
ipv6_6=$(get_mesh_ipv6 "e2e-flood-6")
ipv6_7=$(get_mesh_ipv6 "e2e-flood-7")
assert_can_ping "e2e-flood-6" "$ipv6_7"

TOTAL_TIME=$(($(date +%s) - START_TIME))
info "Total test time: ${TOTAL_TIME}s (join: ${JOIN_TIME}s, converge: ${CONVERGE_TIME}s)"

cleanup
summary
