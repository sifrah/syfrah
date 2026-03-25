#!/usr/bin/env bash
# Scenario: syfrah fabric status shows region and zone

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Status Display ──"

create_network
start_node "e2e-zstat-1" "172.20.0.10"

init_mesh "e2e-zstat-1" "172.20.0.10" "node-1"
sleep 2

output=$(docker exec "e2e-zstat-1" syfrah fabric status 2>&1)

if echo "$output" | grep -q "Region:"; then
    pass "status shows Region field"
else
    fail "status missing Region field"
    echo "$output"
fi

if echo "$output" | grep -q "zone:"; then
    pass "status shows zone field"
else
    fail "status missing zone field"
    echo "$output"
fi

# Region should be default
if echo "$output" | grep -q "Region:.*default"; then
    pass "default region is default"
else
    fail "unexpected default region"
fi

cleanup
summary
