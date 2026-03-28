#!/usr/bin/env bash
# Scenario: Network partition between two nodes, then healing
# Verifies WireGuard keepalive reconnects after partition heals

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Network Partition + Healing ──"

create_network

start_node "e2e-part-1" "172.20.0.10"
start_node "e2e-part-2" "172.20.0.11"
start_node "e2e-part-3" "172.20.0.12"

init_mesh "e2e-part-1" "172.20.0.10" "node-1"
start_peering "e2e-part-1"
join_mesh "e2e-part-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-part-3" "172.20.0.10" "172.20.0.12" "node-3"

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-part-1" 2 30

ipv6_2=$(get_mesh_ipv6 "e2e-part-2")
ipv6_3=$(get_mesh_ipv6 "e2e-part-3")
if [ -z "$ipv6_2" ] || [ -z "$ipv6_3" ]; then
    fail "could not get mesh IPv6 (ipv6_2=$ipv6_2, ipv6_3=$ipv6_3)"
fi

# Verify baseline connectivity
assert_can_ping "e2e-part-1" "$ipv6_2"

# Partition: block traffic between node-1 and node-2
info "Partitioning node-1 <-> node-2..."
block_traffic "e2e-part-1" "172.20.0.11"
block_traffic "e2e-part-2" "172.20.0.10"
sleep 3

# During partition: node-1 cannot reach node-2
assert_cannot_ping "e2e-part-1" "$ipv6_2"

# But node-1 can still reach node-3
assert_can_ping "e2e-part-1" "$ipv6_3"

# Heal partition
info "Healing partition..."
unblock_traffic "e2e-part-1" "172.20.0.11"
unblock_traffic "e2e-part-2" "172.20.0.10"

# Poll for WireGuard keepalive to reconnect (timeout 60s)
_heal_deadline=$(($(date +%s) + 60))
_heal_ok=false
while [ "$(date +%s)" -lt "$_heal_deadline" ]; do
    if docker exec "e2e-part-1" ping -6 -c 1 -W 2 "$ipv6_2" >/dev/null 2>&1; then
        _heal_ok=true
        break
    fi
    sleep 5
done

# After healing: full connectivity restored
if [ "$_heal_ok" = true ]; then
    pass "e2e-part-1 can ping $ipv6_2"
else
    assert_can_ping "e2e-part-1" "$ipv6_2"
fi

cleanup
summary
