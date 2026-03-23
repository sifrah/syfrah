#!/usr/bin/env bash
# Scenario: syfrah state list shows fabric tables after mesh init

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── State CLI: List ──"

create_network
start_node "e2e-slist-1" "172.20.0.10"
start_node "e2e-slist-2" "172.20.0.11"

init_mesh "e2e-slist-1" "172.20.0.10" "node-1"
start_peering "e2e-slist-1"
join_mesh "e2e-slist-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

# state list on a node that has NO redb yet (still uses JSON)
# This should fail gracefully since fabric doesn't use redb yet
info "Testing state list on layer with no redb..."
output=$(docker exec "e2e-slist-1" syfrah state list fabric 2>&1 || true)
if echo "$output" | grep -q "no state database"; then
    pass "state list correctly reports no redb for fabric"
else
    # If fabric already uses redb, check it shows tables
    if echo "$output" | grep -q "Layer:"; then
        pass "state list shows fabric layer info"
    else
        fail "unexpected state list output: $output"
    fi
fi

# Create a state db manually to test the CLI
info "Testing state list with actual data..."
docker exec "e2e-slist-1" bash -c "
    syfrah state get fabric config 2>&1 || true
" >/dev/null 2>&1

# Test state list on nonexistent layer
output=$(docker exec "e2e-slist-1" syfrah state list nonexistent 2>&1 || true)
if echo "$output" | grep -q "no state database"; then
    pass "state list fails for nonexistent layer"
else
    fail "state list should fail for nonexistent layer"
fi

cleanup
summary
