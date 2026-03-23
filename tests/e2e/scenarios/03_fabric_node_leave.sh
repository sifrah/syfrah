#!/usr/bin/env bash
# Scenario: a node stops its daemon, remaining nodes continue
#
# Verifies:
# - Node can be stopped cleanly
# - Remaining nodes still have their interface and connectivity

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Node Stop ──"

create_network

start_node "e2e-leave-1" "172.20.0.10"
start_node "e2e-leave-2" "172.20.0.11"
start_node "e2e-leave-3" "172.20.0.12"

init_mesh "e2e-leave-1" "172.20.0.10" "node-1"
start_peering "e2e-leave-1"
join_mesh "e2e-leave-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-leave-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3

# All 3 nodes connected
assert_peer_count "e2e-leave-1" 2

# Stop node-3 daemon (not leave — just stop)
info "Stopping node-3 daemon..."
stop_daemon "e2e-leave-3"
sleep 2

# Node-1 and Node-2 still running and connected
assert_daemon_running "e2e-leave-1"
assert_daemon_running "e2e-leave-2"
assert_interface_exists "e2e-leave-1"
assert_interface_exists "e2e-leave-2"

# Connectivity between remaining nodes
ipv6_2=$(get_mesh_ipv6 "e2e-leave-2")
assert_can_ping "e2e-leave-1" "$ipv6_2"

cleanup
summary
