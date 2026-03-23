#!/usr/bin/env bash
# Scenario: 3 nodes form a WireGuard mesh via CLI
#
# Verifies:
# - All daemons start successfully
# - Each node sees 2 peers
# - syfrah0 interface exists on all nodes

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Mesh Formation (3 nodes) ──"

create_network

start_node "e2e-form-1" "172.20.0.10"
start_node "e2e-form-2" "172.20.0.11"
start_node "e2e-form-3" "172.20.0.12"

init_mesh "e2e-form-1" "172.20.0.10" "node-1"
start_peering "e2e-form-1"
join_mesh "e2e-form-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-form-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3

assert_daemon_running "e2e-form-1"
assert_daemon_running "e2e-form-2"
assert_daemon_running "e2e-form-3"

assert_peer_count "e2e-form-1" 2
assert_peer_count "e2e-form-2" 2
assert_peer_count "e2e-form-3" 2

assert_interface_exists "e2e-form-1"
assert_interface_exists "e2e-form-2"
assert_interface_exists "e2e-form-3"

cleanup
summary
