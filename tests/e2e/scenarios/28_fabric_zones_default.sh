#!/usr/bin/env bash
# Scenario: default region/zone auto-generation
# Init and join without --region/--zone, verify defaults

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Default Generation ──"

create_network

start_node "e2e-zdef-1" "172.20.0.10"
start_node "e2e-zdef-2" "172.20.0.11"
start_node "e2e-zdef-3" "172.20.0.12"

init_mesh "e2e-zdef-1" "172.20.0.10" "node-1"
start_peering "e2e-zdef-1"
join_mesh "e2e-zdef-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-zdef-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3

# Node-1 should have default region and zone-1
status1=$(docker exec "e2e-zdef-1" syfrah fabric status 2>&1)
if echo "$status1" | grep -q "Region:.*default"; then
    pass "node-1 has default region: default"
else
    fail "node-1 missing default region"
    echo "$status1"
fi

if echo "$status1" | grep -q "Zone:.*zone-1"; then
    pass "node-1 has default zone: zone-1"
else
    fail "node-1 missing default zone"
    echo "$status1"
fi

# Peers should show region/zone in the listing
peers_output=$(docker exec "e2e-zdef-1" syfrah fabric peers 2>&1)
if echo "$peers_output" | grep -q "REGION"; then
    pass "peers output shows REGION column"
else
    fail "peers output missing REGION column"
fi

if echo "$peers_output" | grep -q "ZONE"; then
    pass "peers output shows ZONE column"
else
    fail "peers output missing ZONE column"
fi

cleanup
summary
