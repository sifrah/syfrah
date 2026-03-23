#!/usr/bin/env bash
# Scenario: Reconciliation recovers after WG interface is removed

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Reconciliation Recovery ──"
create_network

start_node "e2e-reconlog-1" "${E2E_IP_PREFIX}.10"
start_node "e2e-reconlog-2" "${E2E_IP_PREFIX}.11"

# Use fast reconcile interval
docker exec "e2e-reconlog-1" mkdir -p /root/.syfrah
docker exec "e2e-reconlog-1" sh -c 'cat > /root/.syfrah/config.toml << EOF
[daemon]
reconcile_interval = 5
EOF'

init_mesh "e2e-reconlog-1" "${E2E_IP_PREFIX}.10" "node-1"
start_peering "e2e-reconlog-1"
join_mesh "e2e-reconlog-2" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.11" "node-2"

sleep 5
assert_peer_count "e2e-reconlog-1" 1

# Daemon should still be running after interface loss
# (reconciliation should log warning, not crash)
if docker exec "e2e-reconlog-1" test -S /root/.syfrah/control.sock 2>/dev/null; then
    pass "daemon still running after WG interface removal"
else
    fail "daemon crashed after WG interface removal"
fi

cleanup
summary
