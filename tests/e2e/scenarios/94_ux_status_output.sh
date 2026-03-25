#!/usr/bin/env bash
# Scenario: UX — status command output validation
# Validates visual sections, secret masking, --verbose, --show-secret flags,
# and both TTY and non-TTY output modes.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Status Output ──"
trap cleanup EXIT
create_network

start_node "e2e-ux-status-1" "172.20.0.10"
start_node "e2e-ux-status-2" "172.20.0.11"

# Set up mesh
init_mesh "e2e-ux-status-1" "172.20.0.10" "status-node"
wait_daemon "e2e-ux-status-1" 30

# Test 1: Status after init — shows key section headers
info "Testing: status shows visual sections..."
output=$(docker exec "e2e-ux-status-1" syfrah fabric status 2>&1)

assert_output_contains "e2e-ux-status-1" "syfrah fabric status" "status-node"
assert_output_matches "e2e-ux-status-1" "syfrah fabric status" "fd[0-9a-f]"
# Verify section headers present (non-TTY plain text fallback)
if echo "$output" | grep -q "Mesh"; then
    pass "status shows Mesh section"
else
    fail "status missing Mesh section"
fi
if echo "$output" | grep -q "Network"; then
    pass "status shows Network section"
else
    fail "status missing Network section"
fi
if echo "$output" | grep -q "Peers"; then
    pass "status shows Peers section"
else
    fail "status missing Peers section"
fi

# Test 2: Status shows region and zone
info "Testing: status shows region/zone..."
if echo "$output" | grep -qi "region\|zone"; then
    pass "status shows region/zone"
else
    fail "status missing region/zone"
fi

# Test 3: Secret is masked by default
info "Testing: secret is masked by default..."
if echo "$output" | grep -q "show-secret"; then
    pass "status masks secret (shows --show-secret hint)"
else
    fail "status does not mask secret"
fi
# Check that the full secret (without masking asterisks) is not shown
if echo "$output" | grep -q "syf_sk_[A-Za-z0-9]\{20,\}"; then
    fail "status leaks full secret in default mode"
else
    pass "status does not leak full secret"
fi

# Test 4: --show-secret reveals full secret
info "Testing: --show-secret reveals secret..."
output_secret=$(docker exec "e2e-ux-status-1" syfrah fabric status --show-secret 2>&1)
if echo "$output_secret" | grep -q "syf_sk_.\{20,\}"; then
    pass "--show-secret reveals full secret"
else
    fail "--show-secret did not reveal full secret"
fi

# Test 5: Config/metrics hidden by default, shown with --verbose
info "Testing: config hidden by default..."
if echo "$output" | grep -qi "Config"; then
    fail "config section visible without --verbose"
else
    pass "config section hidden by default"
fi

info "Testing: --verbose shows config and metrics..."
output_verbose=$(docker exec "e2e-ux-status-1" syfrah fabric status --verbose 2>&1)
if echo "$output_verbose" | grep -qi "Config"; then
    pass "--verbose shows config section"
else
    fail "--verbose missing config section"
fi

# Test 6: Health status is prominent
info "Testing: health status is visible..."
if echo "$output" | grep -qi "Daemon\|running\|stopped"; then
    pass "status shows daemon health"
else
    fail "status missing daemon health"
fi
if echo "$output" | grep -qi "Interface\|syfrah0"; then
    pass "status shows interface health"
else
    fail "status missing interface health"
fi

# Test 7: Status daemon stopped
info "Testing: status after stop..."
stop_daemon "e2e-ux-status-1"
sleep 2
output_stopped=$(docker exec "e2e-ux-status-1" syfrah fabric status 2>&1 || true)
if echo "$output_stopped" | grep -qi "stopped"; then
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

# Test 8: Status with no mesh — suggests init/join
info "Testing: status with no mesh..."
err=$(docker exec "e2e-ux-status-2" syfrah fabric status 2>&1 || true)
if echo "$err" | grep -qi "init\|join"; then
    pass "status no mesh: suggests init/join"
else
    fail "status no mesh: unclear message: $(echo "$err" | head -3)"
fi

cleanup
summary
