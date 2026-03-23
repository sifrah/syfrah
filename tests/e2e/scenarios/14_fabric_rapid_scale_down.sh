#!/usr/bin/env bash
# Scenario: Kill half the nodes abruptly, remaining nodes survive

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Rapid Scale-Down ──"

create_network

for i in $(seq 1 6); do
    start_node "e2e-down-$i" "172.20.0.$((9+i))"
done

init_mesh "e2e-down-1" "172.20.0.10" "node-1"
start_peering "e2e-down-1"

for i in $(seq 2 6); do
    join_mesh "e2e-down-$i" "172.20.0.10" "172.20.0.$((9+i))" "node-$i"
    sleep 1
done

sleep 5
assert_peer_count "e2e-down-1" 5

# Kill nodes 4, 5, 6 abruptly
info "Killing nodes 4, 5, 6..."
for i in 4 5 6; do
    docker rm -f "e2e-down-$i" >/dev/null 2>&1 &
done
wait
# Remove from tracking so cleanup doesn't error
E2E_CONTAINERS=("e2e-down-1" "e2e-down-2" "e2e-down-3")

sleep 5

# Remaining nodes must still be alive
assert_daemon_running "e2e-down-1"
assert_daemon_running "e2e-down-2"
assert_daemon_running "e2e-down-3"

# Remaining nodes can still talk to each other
ipv6_2=$(get_mesh_ipv6 "e2e-down-2")
ipv6_3=$(get_mesh_ipv6 "e2e-down-3")
assert_can_ping "e2e-down-1" "$ipv6_2"
assert_can_ping "e2e-down-1" "$ipv6_3"
assert_can_ping "e2e-down-2" "$ipv6_3"

# State files still valid
assert_state_exists "e2e-down-1"
assert_state_exists "e2e-down-2"
assert_state_exists "e2e-down-3"

cleanup
summary
