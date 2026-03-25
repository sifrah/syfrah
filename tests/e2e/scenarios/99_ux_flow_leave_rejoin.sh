#!/usr/bin/env bash
# Scenario: UX Flow — Leave and rejoin
# Validates leave fully cleans state, rejoin is seamless.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: Leave & Rejoin ──"
trap cleanup EXIT
create_network

start_node "e2e-flow-lr-1" "172.20.0.10"
start_node "e2e-flow-lr-2" "172.20.0.11"

# Setup 2-node mesh
info "Setting up 2-node mesh..."
init_mesh "e2e-flow-lr-1" "172.20.0.10" "lr-server-1"
start_peering "e2e-flow-lr-1"
join_mesh "e2e-flow-lr-2" "172.20.0.10" "172.20.0.11" "lr-server-2"

sleep 5
assert_peer_count "e2e-flow-lr-1" 1
assert_peer_count "e2e-flow-lr-2" 1

# Step 1: Server 2 leaves
info "Step 1: Server 2 leaves..."
output_leave=$(docker exec "e2e-flow-lr-2" syfrah fabric leave --yes 2>&1 || true)
if echo "$output_leave" | grep -qi "clear\|removed\|left\|clean"; then
    pass "leave: clean message"
else
    fail "leave: unclear message: $(echo "$output_leave" | head -3)"
fi

sleep 2

# Step 2: Verify state is clean
info "Step 2: Verify clean state after leave..."
assert_clean_state "e2e-flow-lr-2"

# Step 3: Server 2 rejoins — must work first try
info "Step 3: Server 2 rejoins..."
start_peering "e2e-flow-lr-1"
join_mesh "e2e-flow-lr-2" "172.20.0.10" "172.20.0.11" "lr-server-2"

sleep 5

# Step 4: Both nodes see each other after rejoin
info "Step 4: Peers visible after rejoin..."
output_lr1=$(docker exec "e2e-flow-lr-1" syfrah fabric peers 2>&1)
if echo "$output_lr1" | grep -q "lr-server-2"; then
    pass "server-1 sees server-2 after rejoin"
else
    fail "server-1 doesn't see server-2 after rejoin"
fi

output_lr2=$(docker exec "e2e-flow-lr-2" syfrah fabric peers 2>&1)
if echo "$output_lr2" | grep -q "lr-server-1"; then
    pass "server-2 sees server-1 after rejoin"
else
    fail "server-2 doesn't see server-1 after rejoin"
fi

# Step 5: At least 1 active peer each
info "Step 5: Correct peer counts..."
actual_1=$(docker exec "e2e-flow-lr-1" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
if [ "$actual_1" -ge 1 ]; then
    pass "server-1 has $actual_1 active peer(s)"
else
    fail "server-1 has 0 active peers"
fi
actual_2=$(docker exec "e2e-flow-lr-2" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
if [ "$actual_2" -ge 1 ]; then
    pass "server-2 has $actual_2 active peer(s)"
else
    fail "server-2 has 0 active peers"
fi

# No epoch dates
assert_no_epoch_dates "e2e-flow-lr-1"
assert_no_epoch_dates "e2e-flow-lr-2"

cleanup
summary
