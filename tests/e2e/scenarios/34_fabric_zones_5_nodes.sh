#!/usr/bin/env bash
# Scenario: 5 nodes with default zones — all unique

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: 5 Nodes All Unique ──"

create_network

for i in $(seq 1 5); do
    start_node "e2e-z5-$i" "${E2E_IP_PREFIX}.$((9+i))"
done

init_mesh "e2e-z5-1" "${E2E_IP_PREFIX}.10" "node-1"
start_peering "e2e-z5-1"

for i in $(seq 2 5); do
    # Wait until leader sees expected number of peers before joining next node
    expected=$((i - 2))
    if [ "$expected" -gt 0 ]; then
        for attempt in $(seq 1 15); do
            count=$(docker exec "e2e-z5-1" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
            if [ "$count" -ge "$expected" ]; then
                debug "leader sees $count peer(s), proceeding with node-$i"
                break
            fi
            sleep 1
        done
    fi
    join_mesh "e2e-z5-$i" "${E2E_IP_PREFIX}.10" "${E2E_IP_PREFIX}.$((9+i))" "node-$i"
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
