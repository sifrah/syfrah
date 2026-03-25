#!/usr/bin/env bash
# Scenario: UX — status command output validation
# Validates what the user sees after running syfrah fabric status.

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

# Test 1: Status after init — shows key info in sections
info "Testing: status after init..."
output=$(docker exec "e2e-ux-status-1" syfrah fabric status 2>&1)

assert_output_contains "e2e-ux-status-1" "syfrah fabric status" "status-node"
assert_output_matches "e2e-ux-status-1" "syfrah fabric status" "fd[0-9a-f]"
assert_output_matches "e2e-ux-status-1" "syfrah fabric status" "Daemon"

# Test 2: Non-TTY output uses plain text (piped, no box-drawing)
info "Testing: non-TTY fallback..."
output_piped=$(docker exec "e2e-ux-status-1" sh -c "syfrah fabric status 2>&1 | cat")
if echo "$output_piped" | grep -q "^--- Mesh ---"; then
    pass "non-TTY output uses plain text sections"
else
    fail "non-TTY output missing plain text section headers"
fi

# Test 3: Status shows region and zone
info "Testing: status shows region/zone..."
if echo "$output" | grep -qi "region\|zone"; then
    pass "status shows region/zone"
else
    fail "status missing region/zone"
fi

# Test 4: Secret is masked by default
info "Testing: secret is masked by default..."
if echo "$output_piped" | grep -q "syf_sk_.\{20,\}"; then
    fail "secret is fully exposed in default output"
else
    pass "secret is masked by default"
fi
if echo "$output_piped" | grep -q "\-\-show-secret"; then
    pass "output hints about --show-secret flag"
else
    fail "output missing --show-secret hint"
fi

# Test 5: --show-secret reveals full secret
info "Testing: --show-secret reveals secret..."
output_secret=$(docker exec "e2e-ux-status-1" sh -c "syfrah fabric status --show-secret 2>&1 | cat")
if echo "$output_secret" | grep -q "syf_sk_.\{20,\}"; then
    pass "--show-secret reveals full secret"
else
    fail "--show-secret did not reveal secret"
fi

# Test 6: --verbose shows config and metrics
info "Testing: --verbose shows config/metrics..."
output_verbose=$(docker exec "e2e-ux-status-1" sh -c "syfrah fabric status --verbose 2>&1 | cat")
if echo "$output_verbose" | grep -qi "config\|reconcil"; then
    pass "--verbose shows config section"
else
    fail "--verbose missing config section"
fi

# Test 7: Default output does NOT show config section
info "Testing: default hides config..."
if echo "$output_piped" | grep -qi "reconcile_interval"; then
    fail "default output leaks config section"
else
    pass "config hidden in default output"
fi

# Test 8: Status daemon stopped
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

# Test 9: Status with no mesh — suggests init/join
info "Testing: status with no mesh..."
err=$(docker exec "e2e-ux-status-2" syfrah fabric status 2>&1 || true)
if echo "$err" | grep -qi "init\|join"; then
    pass "status no mesh: suggests init/join"
else
    fail "status no mesh: unclear message: $(echo "$err" | head -3)"
fi

cleanup
summary
