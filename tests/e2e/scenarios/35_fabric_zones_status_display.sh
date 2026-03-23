#!/usr/bin/env bash
# Scenario: syfrah fabric status shows region and zone

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Status Display ──"

create_network
start_node "e2e-zstat-1" "${E2E_IP_PREFIX}.10"

init_mesh "e2e-zstat-1" "${E2E_IP_PREFIX}.10" "node-1"
sleep 2

output=$(docker exec "e2e-zstat-1" syfrah fabric status 2>&1)

if echo "$output" | grep -q "Region:"; then
    pass "status shows Region field"
else
    fail "status missing Region field"
    echo "$output"
fi

if echo "$output" | grep -q "Zone:"; then
    pass "status shows Zone field"
else
    fail "status missing Zone field"
    echo "$output"
fi

# Region should be region-1 (default)
if echo "$output" | grep -q "region-1"; then
    pass "default region is region-1"
else
    fail "unexpected default region"
fi

cleanup
summary
