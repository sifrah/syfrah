#!/usr/bin/env bash
# Scenario: a node leaves the mesh, remaining nodes continue
#
# Verifies:
# - Node can leave cleanly
# - Remaining nodes still have each other as peers
# - Left node's interface is torn down

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Node Leave ──"

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

# Node-3 leaves
info "Node-3 leaving mesh..."
leave_mesh "e2e-leave-3"

sleep 2

# Node-3 interface should be gone
assert_interface_gone "e2e-leave-3"

# Node-1 and Node-2 still have each other
assert_daemon_running "e2e-leave-1"
assert_daemon_running "e2e-leave-2"

# Note: remaining nodes still show the left peer in their list
# (marked as unreachable after timeout, not immediately removed)
# So we just verify the remaining nodes are still functional
assert_interface_exists "e2e-leave-1"
assert_interface_exists "e2e-leave-2"

# Verify connectivity between remaining nodes
ipv6_2=$(get_mesh_ipv6 "e2e-leave-2")
assert_can_ping "e2e-leave-1" "$ipv6_2"

cleanup
summary
