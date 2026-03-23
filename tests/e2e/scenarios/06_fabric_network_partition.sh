#!/usr/bin/env bash
# Scenario: Network partition between two nodes, then healing
# Verifies WireGuard keepalive reconnects after partition heals

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Network Partition + Healing ──"

create_network

start_node "e2e-part-1" "${E2E_IP_PREFIX}.10"
start_node "e2e-part-2" "${E2E_IP_PREFIX}.11"
start_node "e2e-part-3" "${E2E_IP_PREFIX}.12"

init_mesh "e2e-part-1" "${E2E_IP_PREFIX}.10" "node-1"
start_peering "e2e-part-1"
join_mesh "e2e-part-2" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.11" "node-2"
join_mesh "e2e-part-3" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.12" "node-3"

sleep 3

ipv6_2=$(get_mesh_ipv6 "e2e-part-2")
ipv6_3=$(get_mesh_ipv6 "e2e-part-3")

# Verify baseline connectivity
assert_can_ping "e2e-part-1" "$ipv6_2"

# Partition: block traffic between node-1 and node-2
info "Partitioning node-1 <-> node-2..."
block_traffic "e2e-part-1" "${E2E_IP_PREFIX}.11"
block_traffic "e2e-part-2" "${E2E_IP_PREFIX}.10"
sleep 3

# During partition: node-1 cannot reach node-2
assert_cannot_ping "e2e-part-1" "$ipv6_2"

# But node-1 can still reach node-3
assert_can_ping "e2e-part-1" "$ipv6_3"

# Heal partition
info "Healing partition..."
unblock_traffic "e2e-part-1" "${E2E_IP_PREFIX}.11"
unblock_traffic "e2e-part-2" "${E2E_IP_PREFIX}.10"

# Wait for WireGuard keepalive to reconnect (25s interval)
sleep 30

# After healing: full connectivity restored
assert_can_ping "e2e-part-1" "$ipv6_2"

cleanup
summary
