#!/usr/bin/env bash
# Scenario: 5 nodes with default zones — all unique

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: 5 Nodes All Unique ──"

create_network

for i in $(seq 1 5); do
    start_node "e2e-z5-$i" "172.20.0.$((9+i))"
done

init_mesh "e2e-z5-1" "172.20.0.10" "node-1"
start_peering "e2e-z5-1"

for i in $(seq 2 5); do
    join_mesh "e2e-z5-$i" "172.20.0.10" "172.20.0.$((9+i))" "node-$i"
    sleep 3  # wait for peer record to propagate before next join
done

sleep 5

# Collect all zones
zones=()
for i in $(seq 1 5); do
    z=$(docker exec "e2e-z5-$i" syfrah fabric status 2>&1 | grep "Zone:" | awk '{print $2}')
    zones+=("$z")
    info "node-$i zone: $z"
done

# Check all unique
unique_count=$(printf '%s\n' "${zones[@]}" | sort -u | wc -l | tr -d ' ')
if [ "$unique_count" -eq 5 ]; then
    pass "all 5 zones are unique"
else
    fail "only $unique_count unique zones out of 5"
fi

# Check all share region-1
for i in $(seq 1 5); do
    r=$(docker exec "e2e-z5-$i" syfrah fabric status 2>&1 | grep "Region:" | awk '{print $2}')
    if [ "$r" = "region-1" ]; then
        pass "node-$i region: region-1"
    else
        fail "node-$i region: $r (expected region-1)"
    fi
done

cleanup
summary
