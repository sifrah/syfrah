#!/usr/bin/env bash
# Scenario: zone auto-increments as nodes join

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Auto Increment ──"

create_network

start_node "e2e-zinc-1" "172.20.0.10"
start_node "e2e-zinc-2" "172.20.0.11"
start_node "e2e-zinc-3" "172.20.0.12"

init_mesh "e2e-zinc-1" "172.20.0.10" "node-1"
start_peering "e2e-zinc-1"
join_mesh "e2e-zinc-2" "172.20.0.10" "172.20.0.11" "node-2"

# Wait until leader sees node-2 before next join (zone generation depends on peer list)
for attempt in $(seq 1 15); do
    count=$(docker exec "e2e-zinc-1" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
    if [ "$count" -ge 1 ]; then
        debug "leader sees $count peer(s) after ${attempt}s"
        break
    fi
    sleep 1
done

# Debug: show leader's state before node-3 joins
debug "leader peers before node-3 join:"
docker exec "e2e-zinc-1" syfrah fabric peers 2>&1 || true
debug "leader state.json peer count:"
docker exec "e2e-zinc-1" cat /root/.syfrah/state.json 2>/dev/null | jq '.peers | length' || echo "(no json)"

join_mesh "e2e-zinc-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3

# Each node should have a unique zone
z1=$(docker exec "e2e-zinc-1" syfrah fabric status 2>&1 | grep -i "Zone:" | awk '{print $NF}')
z2=$(docker exec "e2e-zinc-2" syfrah fabric status 2>&1 | grep -i "Zone:" | awk '{print $NF}')
z3=$(docker exec "e2e-zinc-3" syfrah fabric status 2>&1 | grep -i "Zone:" | awk '{print $NF}')

info "Zones: $z1, $z2, $z3"

# All zones should be different
if [ "$z1" != "$z2" ] && [ "$z1" != "$z3" ] && [ "$z2" != "$z3" ]; then
    pass "all 3 nodes have unique zones"
else
    fail "zone collision: $z1, $z2, $z3"
fi

# All should share the same region
r1=$(docker exec "e2e-zinc-1" syfrah fabric status 2>&1 | grep "Region:" | awk '{print $2}')
r2=$(docker exec "e2e-zinc-2" syfrah fabric status 2>&1 | grep "Region:" | awk '{print $2}')
r3=$(docker exec "e2e-zinc-3" syfrah fabric status 2>&1 | grep "Region:" | awk '{print $2}')

if [ "$r1" = "$r2" ] && [ "$r2" = "$r3" ]; then
    pass "all 3 nodes share the same region: $r1"
else
    fail "regions differ: $r1, $r2, $r3"
fi

cleanup
summary
