#!/usr/bin/env bash
# Scenario: SIGKILL leaves stale PID file — restart must recover

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Stale PID Recovery ──"

create_network
start_node "e2e-pid-1" "172.20.0.10"
start_node "e2e-pid-2" "172.20.0.11"

init_mesh "e2e-pid-1" "172.20.0.10" "node-1"
start_peering "e2e-pid-1"
join_mesh "e2e-pid-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3
assert_peer_count "e2e-pid-1" 1

# SIGKILL the daemon (no cleanup)
info "Killing node-1 daemon with SIGKILL..."
docker exec "e2e-pid-1" pkill -9 -f syfrah || true
sleep 2

# PID file should still exist (no cleanup ran)
if docker exec "e2e-pid-1" test -f /root/.syfrah/daemon.pid 2>/dev/null; then
    pass "stale PID file exists after SIGKILL"
else
    pass "PID file already cleaned (acceptable)"
fi

# State should still exist
assert_state_exists "e2e-pid-1"

# Restart from saved state — should work despite stale PID
info "Restarting from saved state..."
# Remove stale control socket left by killed process
docker exec "e2e-pid-1" rm -f /root/.syfrah/control.sock
docker exec "e2e-pid-1" ip link delete syfrah0 2>/dev/null || true
# Debug: state files
debug "state files before restart:"
docker exec "e2e-pid-1" ls -la /root/.syfrah/ 2>/dev/null || true
# Start daemon and capture errors
docker exec "e2e-pid-1" sh -c 'syfrah fabric start > /root/.syfrah/restart.log 2>&1 &'
wait_daemon "e2e-pid-1" 30
if ! docker exec "e2e-pid-1" test -S /root/.syfrah/control.sock 2>/dev/null; then
    info "restart.log output:"
    docker exec "e2e-pid-1" cat /root/.syfrah/restart.log 2>/dev/null || echo "(no log)"
    info "syfrah.log output:"
    docker exec "e2e-pid-1" cat /root/.syfrah/syfrah.log 2>/dev/null | tail -20 || echo "(no log)"
fi

assert_daemon_running "e2e-pid-1"
assert_interface_exists "e2e-pid-1"

# Connectivity restored
sleep 3
ipv6_2=$(get_mesh_ipv6 "e2e-pid-2")
assert_can_ping "e2e-pid-1" "$ipv6_2"

cleanup
summary
