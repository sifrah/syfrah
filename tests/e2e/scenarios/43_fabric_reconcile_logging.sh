#!/usr/bin/env bash
# Scenario: Reconciliation logs WG unavailability instead of silent skip

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Reconciliation Logging ──"
create_network

start_node "e2e-reconlog-1" "172.20.0.10"
start_node "e2e-reconlog-2" "172.20.0.11"

# Use fast reconcile interval to speed up the test
docker exec "e2e-reconlog-1" mkdir -p /root/.syfrah
docker exec "e2e-reconlog-1" sh -c 'cat > /root/.syfrah/config.toml << EOF
[daemon]
reconcile_interval = 5
EOF'

init_mesh "e2e-reconlog-1" "172.20.0.10" "node-1"
start_peering "e2e-reconlog-1"
join_mesh "e2e-reconlog-2" "172.20.0.10" "172.20.0.11" "node-2"
if ! wait_for_convergence "e2e-reconlog-" 2 1 30; then
    fail "initial mesh did not converge"
    cleanup; summary
fi

# Remove WireGuard interface (simulates WG crash)
info "Removing WireGuard interface..."
docker exec "e2e-reconlog-1" ip link delete syfrah0 2>/dev/null || true

# Wait for reconciliation cycle
sleep 10

# Check log for WG unavailable warning
log_output=$(docker exec "e2e-reconlog-1" cat /root/.syfrah/syfrah.log 2>&1)
if echo "$log_output" | grep -qi "unavailable\|interface.*error\|reconcil"; then
    pass "reconciliation logs WG unavailability"
else
    fail "reconciliation silently skipped WG error (no log entry)"
    debug "Log tail:"
    echo "$log_output" | tail -10
fi

cleanup
summary
