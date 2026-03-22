#!/usr/bin/env bash
# Scenario: a daemon stops and restarts, rejoins the mesh
#
# Verifies:
# - Daemon can be stopped cleanly
# - Daemon can restart from saved state
# - Peers are restored after restart

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Daemon Restart ──"

create_network

start_node "e2e-restart-1" "172.20.0.10"
start_node "e2e-restart-2" "172.20.0.11"

init_mesh "e2e-restart-1" "172.20.0.10" "node-1"
start_peering "e2e-restart-1"
join_mesh "e2e-restart-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

assert_peer_count "e2e-restart-1" 1

# Stop node-2 daemon
info "Stopping node-2 daemon..."
stop_daemon "e2e-restart-2"
sleep 1

# Verify node-2 interface is gone
assert_interface_gone "e2e-restart-2"

# Restart node-2 from saved state
info "Restarting node-2 daemon..."
docker exec -d "e2e-restart-2" syfrah fabric start
wait_daemon "e2e-restart-2"

assert_daemon_running "e2e-restart-2"
assert_interface_exists "e2e-restart-2"

# Verify connectivity is restored
sleep 2
ipv6_1=$(get_mesh_ipv6 "e2e-restart-1")
assert_can_ping "e2e-restart-2" "$ipv6_1"

cleanup
summary
