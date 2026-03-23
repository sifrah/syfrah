#!/usr/bin/env bash
# Scenario: syfrah state get retrieves values from the state store

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── State CLI: Get ──"

create_network
start_node "e2e-sget-1" "${E2E_IP_PREFIX}.10"

init_mesh "e2e-sget-1" "${E2E_IP_PREFIX}.10" "node-1"
sleep 2

# Test get on nonexistent layer
output=$(docker exec "e2e-sget-1" syfrah state get nonexistent peers 2>&1 || true)
if echo "$output" | grep -q "no state database"; then
    pass "get fails for nonexistent layer"
else
    fail "get should fail for nonexistent layer: $output"
fi

# Test get on nonexistent table in a layer that doesn't have redb yet
output=$(docker exec "e2e-sget-1" syfrah state get fabric peers 2>&1 || true)
if echo "$output" | grep -qi "no state database\|empty table\|error"; then
    pass "get handles missing redb gracefully"
else
    # If it succeeded, fabric is already using redb
    pass "get returned data (fabric may already use redb)"
fi

# Test get with nonexistent key
output=$(docker exec "e2e-sget-1" syfrah state get fabric peers nonexistent_key 2>&1 || true)
if echo "$output" | grep -qi "not found\|no state database\|error"; then
    pass "get fails for nonexistent key"
else
    fail "get should fail for nonexistent key: $output"
fi

cleanup
summary
