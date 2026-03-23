#!/usr/bin/env bash
# Scenario: syfrah fabric peers displays region/zone columns

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Peers Display ──"

create_network

start_node "e2e-zdisp-1" "172.20.0.10"
start_node "e2e-zdisp-2" "172.20.0.11"

# Init with known region/zone
docker exec -d "e2e-zdisp-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-zone-1

wait_daemon "e2e-zdisp-1"
start_peering "e2e-zdisp-1"

join_mesh "e2e-zdisp-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

# Check peers output has REGION and ZONE column headers
output=$(docker exec "e2e-zdisp-1" syfrah fabric peers 2>&1)

if echo "$output" | grep -q "REGION"; then
    pass "peers output shows REGION column header"
else
    fail "peers output missing REGION column header"
    echo "$output"
fi

if echo "$output" | grep -q "ZONE"; then
    pass "peers output shows ZONE column header"
else
    fail "peers output missing ZONE column header"
    echo "$output"
fi

# Check that the leader sees the joiner's region/zone (not dashes)
if echo "$output" | grep "node-2" | grep -q "region-1"; then
    pass "leader sees joiner's region (region-1)"
else
    fail "leader does not see joiner's region"
    echo "$output"
fi

# The joiner gets an auto-generated zone; verify it is not a dash
if echo "$output" | grep "node-2" | grep -q "region-1-zone-"; then
    pass "leader sees joiner's zone"
else
    fail "leader does not see joiner's zone"
    echo "$output"
fi

# Check that node-1's status shows its own region/zone
status1=$(docker exec "e2e-zdisp-1" syfrah fabric status 2>&1)
if echo "$status1" | grep -q "eu-west"; then
    pass "node-1 status shows its own region eu-west"
else
    fail "node-1 status missing region"
    echo "$status1"
fi

# Check that the joiner sees the leader's region/zone
output2=$(docker exec "e2e-zdisp-2" syfrah fabric peers 2>&1)
if echo "$output2" | grep "node-1" | grep -q "eu-west"; then
    pass "joiner sees leader's region (eu-west)"
else
    fail "joiner does not see leader's region"
    echo "$output2"
fi

if echo "$output2" | grep "node-1" | grep -q "eu-west-zone-1"; then
    pass "joiner sees leader's zone (eu-west-zone-1)"
else
    fail "joiner does not see leader's zone"
    echo "$output2"
fi

cleanup
summary
