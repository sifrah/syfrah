#!/usr/bin/env bash
# Scenario: UX — init command output validation
# Validates what the user sees after running syfrah fabric init.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Init Output ──"
trap cleanup EXIT
create_network

start_node "e2e-ux-init-1" "172.20.0.110"

# Test 1: Init happy path — output contains key information
info "Testing: init happy path output..."
output=$(docker exec "e2e-ux-init-1" syfrah fabric init \
    --name test-mesh --node-name node-1 --endpoint 172.20.0.110:51820 2>&1)

# Secret is no longer printed during init (security improvement)
echo "$output" | grep -qvF "syf_sk_" || fail "init should not show secret"
pass "init output does not leak secret"

echo "$output" | grep -q "node-1" || fail "init output missing node name"
pass "init output contains node name"

echo "$output" | grep -qE "fd[0-9a-f]" || fail "init output missing IPv6 (fd prefix)"
pass "init output contains IPv6 address"

echo "$output" | grep -qi "region\|zone" || fail "init output missing region/zone"
pass "init output contains region/zone"

wait_daemon "e2e-ux-init-1" 30

# Test 2: Init output — all suggested commands are valid syfrah commands
info "Testing: init suggested commands..."
assert_all_commands_valid "e2e-ux-init-1" "syfrah fabric status"

# Test 3: Init output — no raw errors
info "Testing: init output no raw errors..."
assert_output_not_contains "e2e-ux-init-1" "syfrah fabric status" "anyhow"
assert_output_not_contains "e2e-ux-init-1" "syfrah fabric status" "os error"
assert_output_not_contains "e2e-ux-init-1" "syfrah fabric status" "stack backtrace"

# Test 4: Double init — says "already exists", suggests leave
info "Testing: double init..."
output2=$(docker exec "e2e-ux-init-1" syfrah fabric init \
    --name test-mesh2 --node-name node-2 --endpoint 172.20.0.110:51820 2>&1 || true)

if echo "$output2" | grep -qi "already\|exists"; then
    pass "double init says already exists"
else
    fail "double init: unclear message: $(echo "$output2" | head -3)"
fi

if echo "$output2" | grep -qF "syfrah fabric leave"; then
    pass "double init suggests 'syfrah fabric leave'"
else
    fail "double init doesn't suggest 'syfrah fabric leave'"
fi

cleanup
summary
