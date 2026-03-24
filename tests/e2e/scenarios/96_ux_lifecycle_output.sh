#!/usr/bin/env bash
# Scenario: UX — lifecycle command output validation
# Validates what the user sees for stop, start, leave, token, rotate.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Lifecycle Output ──"
trap cleanup EXIT
create_network

start_node "e2e-ux-life-1" "172.20.0.10"

# Set up mesh
init_mesh "e2e-ux-life-1" "172.20.0.10" "life-node"
wait_daemon "e2e-ux-life-1" 30

# Test 1: Stop when running — clean message
info "Testing: stop when running..."
output_stop=$(docker exec "e2e-ux-life-1" syfrah fabric stop 2>&1 || true)
if echo "$output_stop" | grep -qi "stop\|shut\|down\|daemon"; then
    pass "stop: clean stop message"
else
    fail "stop: unclear message: $(echo "$output_stop" | head -3)"
fi

sleep 2

# Test 2: Stop when not running — "not running", no error
info "Testing: stop when already stopped..."
output_stop2=$(docker exec "e2e-ux-life-1" syfrah fabric stop 2>&1 || true)
if echo "$output_stop2" | grep -qi "not running\|already\|nothing\|no daemon"; then
    pass "stop when stopped: says not running"
else
    fail "stop when stopped: unclear: $(echo "$output_stop2" | head -3)"
fi

if echo "$output_stop2" | grep -qi "panic\|anyhow\|stack backtrace"; then
    fail "stop when stopped: raw error"
else
    pass "stop when stopped: no raw error"
fi

# Test 3: Start after stop — works
info "Testing: start after stop..."
output_start=$(docker exec "e2e-ux-life-1" syfrah fabric start 2>&1 || true)
# Give daemon time to come up
wait_daemon "e2e-ux-life-1" 30
assert_daemon_running "e2e-ux-life-1"

# Test 4: Token — shows syf_sk_ format
info "Testing: token output..."
output_token=$(docker exec "e2e-ux-life-1" syfrah fabric token 2>&1 || true)
if echo "$output_token" | grep -qF "syf_sk_"; then
    pass "token: shows syf_sk_ format"
else
    fail "token: missing syf_sk_ format: $(echo "$output_token" | head -3)"
fi

# Test 5: Leave — clean message
info "Testing: leave output..."
output_leave=$(docker exec "e2e-ux-life-1" syfrah fabric leave 2>&1 || true)
if echo "$output_leave" | grep -qi "clear\|removed\|left\|clean"; then
    pass "leave: clean message"
else
    fail "leave: unclear message: $(echo "$output_leave" | head -3)"
fi

# Verify no WireGuard warnings in leave output
if echo "$output_leave" | grep -qi "wireguard.*error\|wg.*warn"; then
    fail "leave: WireGuard warnings visible"
else
    pass "leave: no WireGuard warnings"
fi

# Test 6: Double leave — "nothing to do"
info "Testing: double leave..."
output_leave2=$(docker exec "e2e-ux-life-1" syfrah fabric leave 2>&1 || true)
if echo "$output_leave2" | grep -qi "nothing\|already\|no mesh\|not.*found\|no state"; then
    pass "double leave: says nothing to do"
else
    fail "double leave: unclear: $(echo "$output_leave2" | head -3)"
fi

cleanup
summary
