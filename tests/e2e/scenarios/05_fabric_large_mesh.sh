#!/usr/bin/env bash
# Scenario: 5 nodes form a mesh
#
# Verifies:
# - All 5 nodes join successfully
# - Each node sees 4 peers
# - End-to-end connectivity (node-1 ↔ node-5)

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Large Mesh (5 nodes) ──"

create_network

start_node "e2e-large-1" "172.20.0.10"
start_node "e2e-large-2" "172.20.0.11"
start_node "e2e-large-3" "172.20.0.12"
start_node "e2e-large-4" "172.20.0.13"
start_node "e2e-large-5" "172.20.0.14"

init_mesh "e2e-large-1" "172.20.0.10" "node-1"
start_peering "e2e-large-1"

# Join sequentially with pauses to let announcements propagate
join_mesh "e2e-large-2" "172.20.0.10" "172.20.0.11" "node-2"
sleep 1
join_mesh "e2e-large-3" "172.20.0.10" "172.20.0.12" "node-3"
sleep 1
join_mesh "e2e-large-4" "172.20.0.10" "172.20.0.13" "node-4"
sleep 1
join_mesh "e2e-large-5" "172.20.0.10" "172.20.0.14" "node-5"

# Wait for all peer announcements to propagate
sleep 8

for i in 1 2 3 4 5; do
    assert_peer_count "e2e-large-$i" 4
done

# Spot check connectivity (node-1 ↔ node-5)
ipv6_5=$(get_mesh_ipv6 "e2e-large-5")
assert_can_ping "e2e-large-1" "$ipv6_5"

ipv6_1=$(get_mesh_ipv6 "e2e-large-1")
assert_can_ping "e2e-large-5" "$ipv6_1"

cleanup
summary
