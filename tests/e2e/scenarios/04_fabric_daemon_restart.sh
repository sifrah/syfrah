#!/usr/bin/env bash
# Scenario: a daemon restarts from saved state and reconnects
#
# Verifies:
# - Daemon can start from saved state
# - Peers are restored after restart
# - Connectivity works after restart

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Daemon Restart ──"

create_network

start_node "e2e-restart-1" "172.20.0.10"
start_node "e2e-restart-2" "172.20.0.11"

init_mesh "e2e-restart-1" "172.20.0.10" "node-1"
start_peering "e2e-restart-1"
join_mesh "e2e-restart-2" "172.20.0.10" "172.20.0.11" "node-2"

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-restart-1" 1 30

assert_peer_count "e2e-restart-1" 1

# Kill the syfrah process on node-2 (simulates crash)
info "Killing node-2 daemon process..."
docker exec "e2e-restart-2" pkill -f syfrah || true
sleep 2

# Restart node-2 from saved state
info "Restarting node-2 daemon..."
# Remove stale control socket and WG interface left by killed process
docker exec "e2e-restart-2" rm -f /root/.syfrah/control.sock /root/.syfrah/daemon.pid
docker exec "e2e-restart-2" ip link delete syfrah0 2>/dev/null || true
# Debug: verify state exists and try start in foreground briefly
debug "state files before restart:"
docker exec "e2e-restart-2" ls -la /root/.syfrah/ 2>/dev/null || true
# Start daemon and capture any immediate errors
docker exec "e2e-restart-2" sh -c 'syfrah fabric start > /root/.syfrah/restart.log 2>&1 &'
wait_daemon "e2e-restart-2" 30
if ! docker exec "e2e-restart-2" test -S /root/.syfrah/control.sock 2>/dev/null; then
    info "restart.log output:"
    docker exec "e2e-restart-2" cat /root/.syfrah/restart.log 2>/dev/null || echo "(no log)"
    info "syfrah.log output:"
    docker exec "e2e-restart-2" cat /root/.syfrah/syfrah.log 2>/dev/null | tail -20 || echo "(no log)"
fi

assert_daemon_running "e2e-restart-2"

# Verify connectivity is restored
sleep 2
ipv6_1=$(get_mesh_ipv6 "e2e-restart-1")
if [ -n "$ipv6_1" ]; then
    assert_can_ping "e2e-restart-2" "$ipv6_1"
else
    fail "could not get mesh IPv6 for e2e-restart-1"
fi

cleanup
summary
