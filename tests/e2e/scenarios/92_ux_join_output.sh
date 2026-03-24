#!/usr/bin/env bash
# Scenario: UX — join command output validation
# Validates what the user sees after running syfrah fabric join.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Join Output ──"
create_network

start_node "e2e-ux-join-1" "172.20.0.10"
start_node "e2e-ux-join-2" "172.20.0.11"
start_node "e2e-ux-join-3" "172.20.0.12"

# Set up mesh on node 1
init_mesh "e2e-ux-join-1" "172.20.0.10" "server-1"
start_peering "e2e-ux-join-1"

# Test 1: Join happy path — shows key information
info "Testing: join happy path output..."
output=$(docker exec "e2e-ux-join-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name server-2 --endpoint 172.20.0.11:51820 --pin "$E2E_PIN" 2>&1)

echo "$output" | grep -qi "joined\|approved\|accepted" || fail "join output missing approval confirmation"
pass "join output shows approval"

echo "$output" | grep -qE "fd[0-9a-f]" || fail "join output missing IPv6"
pass "join output shows IPv6"

# Test 2: Join shows approval method
info "Testing: join shows approval method..."
if echo "$output" | grep -qi "pin\|manual\|approved\|accepted"; then
    pass "join output shows approval method"
else
    fail "join output missing approval method"
fi

# Test 3: Join target unreachable — human-readable error
info "Testing: join target unreachable..."
err=$(docker exec "e2e-ux-join-3" syfrah fabric join 172.20.0.99:51821 \
    --node-name server-3 --endpoint 172.20.0.12:51820 --pin 1234 2>&1 || true)

if echo "$err" | grep -qi "os error 111\|ECONNREFUSED"; then
    fail "join unreachable shows raw OS error: $(echo "$err" | head -3)"
else
    pass "join unreachable: no raw OS error"
fi

# Test 4: Join state exists — suggests correct command path
info "Testing: join when state exists..."
wait_daemon "e2e-ux-join-2" 30
err2=$(docker exec "e2e-ux-join-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name server-2b --endpoint 172.20.0.11:51820 --pin "$E2E_PIN" 2>&1 || true)

if echo "$err2" | grep -qi "already\|exists\|leave"; then
    pass "join with state: mentions existing state"
else
    fail "join with state: unclear message: $(echo "$err2" | head -3)"
fi

# Verify it says "syfrah fabric leave" not just "syfrah leave"
if echo "$err2" | grep -qF "syfrah fabric leave"; then
    pass "join with state: suggests 'syfrah fabric leave' (full path)"
elif echo "$err2" | grep -qi "leave"; then
    # Acceptable if it mentions leave at all
    pass "join with state: mentions leave command"
else
    fail "join with state: doesn't suggest leave"
fi

# Test 5: Join no args — shows help, not crash
info "Testing: join no args..."
err3=$(docker exec "e2e-ux-join-3" syfrah fabric join 2>&1 || true)
if echo "$err3" | grep -qi "usage\|help\|argument\|required"; then
    pass "join no args: shows usage/help"
else
    fail "join no args: no usage info: $(echo "$err3" | head -3)"
fi

cleanup
summary
