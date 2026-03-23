#!/usr/bin/env bash
# Scenario: Daemon uses custom intervals from config.toml

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Custom Intervals ──"
create_network

start_node "e2e-config-1" "172.20.0.10"
start_node "e2e-config-2" "172.20.0.11"

# Write custom config with fast health check and fast unreachable timeout
docker exec "e2e-config-1" mkdir -p /root/.syfrah
docker exec "e2e-config-1" sh -c 'cat > /root/.syfrah/config.toml << EOF
[daemon]
health_check_interval = 10
unreachable_timeout = 20
reconcile_interval = 10
EOF'

# Init and join with custom config
init_mesh "e2e-config-1" "172.20.0.10" "node-1"
start_peering "e2e-config-1"
join_mesh "e2e-config-2" "172.20.0.10" "172.20.0.11" "node-2"
if ! wait_for_convergence "e2e-config-" 2 1 30; then
    fail "initial mesh did not converge"
    cleanup; summary
fi

# Verify config is displayed in status
config_output=$(docker exec "e2e-config-1" syfrah fabric status 2>&1)
if echo "$config_output" | grep -q "health_check_interval.*10"; then
    pass "status shows custom health_check_interval"
else
    fail "status does not show custom health_check_interval"
    echo "$config_output"
fi

if echo "$config_output" | grep -q "unreachable_timeout.*20"; then
    pass "status shows custom unreachable_timeout"
else
    fail "status does not show custom unreachable_timeout"
    echo "$config_output"
fi

# Block traffic to node-2, peer should be marked unreachable faster
info "Blocking traffic to node-2..."
block_traffic "e2e-config-1" "172.20.0.11"

info "Waiting 35s for fast unreachable detection (20s timeout + 10s check)..."
sleep 35

# Check peer status
peer_status=$(docker exec "e2e-config-1" syfrah fabric peers 2>&1 | grep "node-2")
if echo "$peer_status" | grep -qi "unreachable"; then
    pass "node-2 marked unreachable within 35s (fast config)"
else
    fail "node-2 not yet unreachable after 35s"
    echo "$peer_status"
fi

cleanup
summary
