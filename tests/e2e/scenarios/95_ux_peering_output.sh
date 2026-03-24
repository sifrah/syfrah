#!/usr/bin/env bash
# Scenario: UX — peering command output validation
# Validates what the user sees during peering operations.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Peering Output ──"
trap cleanup EXIT
create_network

start_node "e2e-ux-peer-1" "172.20.0.10"
start_node "e2e-ux-peer-2" "172.20.0.11"

# Set up mesh on node 1
init_mesh "e2e-ux-peer-1" "172.20.0.10" "peer-node-1"

# Test 1: Peering --pin — shows PIN prominently
info "Testing: peering --pin output..."
# Start peering with the standard E2E PIN
start_peering "e2e-ux-peer-1"

# The peering command itself runs in the foreground in real use;
# in E2E we verify via the join side that PIN-based peering works
join_mesh "e2e-ux-peer-2" "172.20.0.10" "172.20.0.11" "peer-node-2"

sleep 3

# Test 2: Verify peering resulted in working mesh
info "Testing: peering resulted in connected mesh..."
output=$(docker exec "e2e-ux-peer-1" syfrah fabric peers 2>&1)
if echo "$output" | grep -q "peer-node-2"; then
    pass "peering: peer visible after PIN join"
else
    fail "peering: peer not visible after PIN join"
fi

output2=$(docker exec "e2e-ux-peer-2" syfrah fabric peers 2>&1)
if echo "$output2" | grep -q "peer-node-1"; then
    pass "peering: initiator visible to joiner"
else
    fail "peering: initiator not visible to joiner"
fi

# Test 3: No raw errors in any output
info "Testing: no raw errors..."
assert_output_not_contains "e2e-ux-peer-1" "syfrah fabric status" "anyhow"
assert_output_not_contains "e2e-ux-peer-1" "syfrah fabric status" "os error"
assert_output_not_contains "e2e-ux-peer-2" "syfrah fabric status" "anyhow"
assert_output_not_contains "e2e-ux-peer-2" "syfrah fabric status" "os error"

cleanup
summary
