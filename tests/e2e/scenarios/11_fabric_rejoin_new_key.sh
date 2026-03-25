#!/usr/bin/env bash
# Scenario: Node leaves and rejoins — gets new WG keypair
# Existing peers must update their WireGuard config

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Rejoin With New Key ──"

create_network

start_node "e2e-rekey-1" "172.20.0.10"
start_node "e2e-rekey-2" "172.20.0.11"
start_node "e2e-rekey-3" "172.20.0.12"

init_mesh "e2e-rekey-1" "172.20.0.10" "node-1"
start_peering "e2e-rekey-1"
join_mesh "e2e-rekey-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-rekey-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3
assert_peer_count "e2e-rekey-1" 2

# Record node-3's original WG key
old_key=$(get_state_field "e2e-rekey-3" ".wg_public_key")
info "Node-3 original key: ${old_key:0:20}..."

# Node-3 leaves
info "Node-3 leaving..."
docker exec "e2e-rekey-3" syfrah fabric leave --yes 2>/dev/null || true
docker exec "e2e-rekey-3" pkill -f syfrah 2>/dev/null || true
sleep 2

# Node-3 rejoins (gets new keypair)
info "Node-3 rejoining..."
join_mesh "e2e-rekey-3" "172.20.0.10" "172.20.0.12" "node-3"

# Verify new key is different
new_key=$(get_state_field "e2e-rekey-3" ".wg_public_key")
if [ "$old_key" != "$new_key" ] && [ -n "$new_key" ]; then
    pass "node-3 has new WG key after rejoin"
else
    fail "node-3 WG key unchanged after rejoin"
fi

# Wait for announcement propagation
sleep 5

# Verify connectivity with new key
ipv6_3=$(get_mesh_ipv6 "e2e-rekey-3")
ipv6_1=$(get_mesh_ipv6 "e2e-rekey-1")
assert_can_ping "e2e-rekey-1" "$ipv6_3"
assert_can_ping "e2e-rekey-3" "$ipv6_1"

cleanup
summary
