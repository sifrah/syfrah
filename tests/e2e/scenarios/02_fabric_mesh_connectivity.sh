#!/usr/bin/env bash
# Scenario: all nodes can ping each other via the WireGuard mesh
#
# Verifies:
# - IPv6 mesh connectivity between all pairs of nodes

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Mesh Connectivity (IPv6 ping) ──"

create_network

start_node "e2e-ping-1" "172.20.0.10"
start_node "e2e-ping-2" "172.20.0.11"
start_node "e2e-ping-3" "172.20.0.12"

init_mesh "e2e-ping-1" "172.20.0.10" "node-1"
start_peering "e2e-ping-1"
join_mesh "e2e-ping-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-ping-3" "172.20.0.10" "172.20.0.12" "node-3"

wait_for_convergence "e2e-ping-" 3 2 30 || true

ipv6_1=$(get_mesh_ipv6 "e2e-ping-1")
ipv6_2=$(get_mesh_ipv6 "e2e-ping-2")
ipv6_3=$(get_mesh_ipv6 "e2e-ping-3")

if [ -z "$ipv6_1" ] || [ -z "$ipv6_2" ] || [ -z "$ipv6_3" ]; then
    fail "could not get mesh IPv6 (ipv6_1=$ipv6_1, ipv6_2=$ipv6_2, ipv6_3=$ipv6_3)"
else
    assert_can_ping "e2e-ping-1" "$ipv6_2"
    assert_can_ping "e2e-ping-1" "$ipv6_3"
    assert_can_ping "e2e-ping-2" "$ipv6_1"
    assert_can_ping "e2e-ping-2" "$ipv6_3"
    assert_can_ping "e2e-ping-3" "$ipv6_1"
    assert_can_ping "e2e-ping-3" "$ipv6_2"
fi

cleanup
summary
