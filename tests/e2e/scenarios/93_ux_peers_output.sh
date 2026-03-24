#!/usr/bin/env bash
# Scenario: UX — peers command output validation
# Validates what the user sees after running syfrah fabric peers.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Peers Output ──"
trap cleanup EXIT
create_network

start_node "e2e-ux-peers-1" "172.20.0.10"
start_node "e2e-ux-peers-2" "172.20.0.11"
start_node "e2e-ux-peers-3" "172.20.0.12"

# Set up 2-node mesh
init_mesh "e2e-ux-peers-1" "172.20.0.10" "alpha"
start_peering "e2e-ux-peers-1"
join_mesh "e2e-ux-peers-2" "172.20.0.10" "172.20.0.11" "bravo"

sleep 5

# Test 1: Peers after join — no duplicate entries
info "Testing: peers no duplicates..."
assert_no_duplicate_peers "e2e-ux-peers-1"
assert_no_duplicate_peers "e2e-ux-peers-2"

# Test 2: Peers region/zone — not empty when data exists
info "Testing: peers region/zone displayed..."
assert_regions_displayed "e2e-ux-peers-1"
assert_regions_displayed "e2e-ux-peers-2"

# Test 3: Peers handshake — no "20535d ago" epoch dates
info "Testing: peers no epoch dates..."
assert_no_epoch_dates "e2e-ux-peers-1"
assert_no_epoch_dates "e2e-ux-peers-2"

# Test 4: Peers names readable — no unnecessary truncation
info "Testing: peers names readable..."
output=$(docker exec "e2e-ux-peers-1" syfrah fabric peers 2>&1)
if echo "$output" | grep -q "bravo"; then
    pass "peer name 'bravo' fully displayed"
else
    fail "peer name 'bravo' truncated or missing: $(echo "$output" | head -5)"
fi

output2=$(docker exec "e2e-ux-peers-2" syfrah fabric peers 2>&1)
if echo "$output2" | grep -q "alpha"; then
    pass "peer name 'alpha' fully displayed"
else
    fail "peer name 'alpha' truncated or missing: $(echo "$output2" | head -5)"
fi

# Test 5: Peers with no mesh — suggests init/join
info "Testing: peers with no mesh..."
err=$(docker exec "e2e-ux-peers-3" syfrah fabric peers 2>&1 || true)
if echo "$err" | grep -qi "init\|join"; then
    pass "peers no mesh: suggests init/join"
else
    fail "peers no mesh: unclear message: $(echo "$err" | head -3)"
fi

cleanup
summary
