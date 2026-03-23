#!/usr/bin/env bash
# Scenario: Full secret rotation flow
# Rotate on leader, all peers rejoin with new secret

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Secret Rotation ──"

create_network

start_node "e2e-rot-1" "172.20.0.10"
start_node "e2e-rot-2" "172.20.0.11"
start_node "e2e-rot-3" "172.20.0.12"

init_mesh "e2e-rot-1" "172.20.0.10" "node-1"
start_peering "e2e-rot-1"
join_mesh "e2e-rot-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-rot-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3
assert_peer_count "e2e-rot-1" 2

# Record original secret
old_secret=$(get_state_field "e2e-rot-1" ".mesh_secret")
info "Old secret: ${old_secret:0:20}..."

# Rotate: must stop daemon first
info "Rotating secret on node-1..."
docker exec "e2e-rot-1" syfrah fabric stop 2>/dev/null || true
docker exec "e2e-rot-1" pkill -f syfrah 2>/dev/null || true
sleep 2
docker exec "e2e-rot-1" syfrah fabric rotate

# Verify new secret
new_secret=$(get_state_field "e2e-rot-1" ".mesh_secret")
if [ "$old_secret" != "$new_secret" ] && [ -n "$new_secret" ]; then
    pass "secret rotated (different from original)"
else
    fail "secret did not change"
fi

# Verify peers cleared
peer_count=$(get_state_field "e2e-rot-1" ".peers | length")
if [ "$peer_count" = "0" ]; then
    pass "peer list cleared after rotation"
else
    fail "peer list not cleared (has $peer_count peers)"
fi

# Restart node-1 with new secret
docker exec -d "e2e-rot-1" syfrah fabric start
wait_daemon "e2e-rot-1"
start_peering "e2e-rot-1"

# Old nodes must leave and rejoin
docker exec "e2e-rot-2" syfrah fabric leave 2>/dev/null || true
docker exec "e2e-rot-2" pkill -f syfrah 2>/dev/null || true
sleep 1
docker exec "e2e-rot-3" syfrah fabric leave 2>/dev/null || true
docker exec "e2e-rot-3" pkill -f syfrah 2>/dev/null || true
sleep 1

join_mesh "e2e-rot-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-rot-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 5

# Verify mesh reformed with new secret
assert_peer_count "e2e-rot-1" 2

# Verify node-2 has the new secret
node2_secret=$(get_state_field "e2e-rot-2" ".mesh_secret")
if [ "$node2_secret" = "$new_secret" ]; then
    pass "node-2 has the new secret"
else
    fail "node-2 has wrong secret"
fi

cleanup
summary
