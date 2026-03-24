#!/usr/bin/env bash
# Scenario: UX Flow — Survive a reboot
# Validates daemon auto-starts and mesh recovers after restart.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: Reboot ──"
trap cleanup EXIT
create_network

start_node "e2e-flow-reboot-1" "172.20.0.10"
start_node "e2e-flow-reboot-2" "172.20.0.11"

# Setup 2-node mesh
info "Setting up 2-node mesh..."
init_mesh "e2e-flow-reboot-1" "172.20.0.10" "reboot-srv-1"
start_peering "e2e-flow-reboot-1"
join_mesh "e2e-flow-reboot-2" "172.20.0.10" "172.20.0.11" "reboot-srv-2"

sleep 5
assert_peer_count "e2e-flow-reboot-1" 1
assert_peer_count "e2e-flow-reboot-2" 1

# Step 1: Reboot server 2 (docker restart)
info "Step 1: Restarting server 2..."
docker restart "e2e-flow-reboot-2"

# Step 2: Wait for daemon to come back (container needs time to restart fully)
info "Step 2: Waiting for daemon recovery..."
sleep 5
wait_daemon "e2e-flow-reboot-2" 60

# Step 3: Daemon is responsive (socket exists, commands work)
info "Step 3: Daemon is responsive..."
if docker exec "e2e-flow-reboot-2" test -S /root/.syfrah/control.sock 2>/dev/null; then
    pass "e2e-flow-reboot-2 daemon socket exists after reboot"
else
    fail "e2e-flow-reboot-2 daemon socket missing after reboot"
fi

# Step 4: Give mesh time to reconverge
sleep 15

# Step 5: Server 2 sees server 1 as active
info "Step 5: Server 2 peers after reboot..."
output_peers2=$(docker exec "e2e-flow-reboot-2" syfrah fabric peers 2>&1)
if echo "$output_peers2" | grep -q "reboot-srv-1"; then
    pass "server-2 sees server-1 after reboot"
else
    fail "server-2 lost server-1 after reboot: $(echo "$output_peers2" | head -5)"
fi

# Step 6: Server 1 sees server 2 as active (not unreachable)
info "Step 6: Server 1 peers after server 2 reboot..."
output_peers1=$(docker exec "e2e-flow-reboot-1" syfrah fabric peers 2>&1)
if echo "$output_peers1" | grep -q "reboot-srv-2"; then
    pass "server-1 sees server-2 after reboot"
else
    fail "server-1 lost server-2 after reboot: $(echo "$output_peers1" | head -5)"
fi

# Final checks
assert_no_duplicate_peers "e2e-flow-reboot-1"
assert_no_duplicate_peers "e2e-flow-reboot-2"
assert_no_epoch_dates "e2e-flow-reboot-1"
assert_no_epoch_dates "e2e-flow-reboot-2"

cleanup
summary
