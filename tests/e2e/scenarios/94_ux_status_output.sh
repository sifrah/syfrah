#!/usr/bin/env bash
# Scenario: UX — status command output validation
# Validates what the user sees after running syfrah fabric status.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Status Output ──"
create_network

start_node "e2e-ux-status-1" "172.20.0.10"
start_node "e2e-ux-status-2" "172.20.0.11"

# Set up mesh
init_mesh "e2e-ux-status-1" "172.20.0.10" "status-node"
wait_daemon "e2e-ux-status-1" 30

# Test 1: Status after init — shows key info
info "Testing: status after init..."
output=$(docker exec "e2e-ux-status-1" syfrah fabric status 2>&1)

assert_output_contains "e2e-ux-status-1" "syfrah fabric status" "status-node"
assert_output_matches "e2e-ux-status-1" "syfrah fabric status" "fd[0-9a-f]"
assert_output_matches "e2e-ux-status-1" "syfrah fabric status" "running\|active"

# Test 2: Status shows region and zone
info "Testing: status shows region/zone..."
if echo "$output" | grep -qi "region\|zone"; then
    pass "status shows region/zone"
else
    fail "status missing region/zone"
fi

# Test 3: Status shows metrics
info "Testing: status shows metrics..."
if echo "$output" | grep -qi "peer\|uptime\|reconcil"; then
    pass "status shows metrics"
else
    fail "status missing metrics info"
fi

# Test 4: Status daemon stopped
info "Testing: status after stop..."
stop_daemon "e2e-ux-status-1"
sleep 2
output_stopped=$(docker exec "e2e-ux-status-1" syfrah fabric status 2>&1 || true)
if echo "$output_stopped" | grep -qi "stopped\|not running"; then
    pass "status shows stopped state"
else
    fail "status after stop: unclear: $(echo "$output_stopped" | head -3)"
fi

# Ensure no crash/raw error
if echo "$output_stopped" | grep -qi "panic\|anyhow\|stack backtrace"; then
    fail "status after stop: shows raw error"
else
    pass "status after stop: no raw errors"
fi

# Test 5: Status with no mesh — suggests init/join
info "Testing: status with no mesh..."
err=$(docker exec "e2e-ux-status-2" syfrah fabric status 2>&1 || true)
if echo "$err" | grep -qi "init\|join"; then
    pass "status no mesh: suggests init/join"
else
    fail "status no mesh: unclear message: $(echo "$err" | head -3)"
fi

cleanup
summary
