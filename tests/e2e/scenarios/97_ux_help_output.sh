#!/usr/bin/env bash
# Scenario: UX — help and version output validation
# Validates what the user sees for --help and --version.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX: Help Output ──"
trap cleanup EXIT
create_network

start_node "e2e-ux-help-1" "172.20.0.10"

# Test 1: syfrah --help lists fabric and state
info "Testing: syfrah --help..."
output=$(docker exec "e2e-ux-help-1" syfrah --help 2>&1 || true)
if echo "$output" | grep -qi "fabric"; then
    pass "syfrah --help: lists fabric"
else
    fail "syfrah --help: missing fabric"
fi
if echo "$output" | grep -qi "state"; then
    pass "syfrah --help: lists state"
else
    fail "syfrah --help: missing state"
fi

# Test 2: syfrah fabric --help lists subcommands
info "Testing: syfrah fabric --help..."
output2=$(docker exec "e2e-ux-help-1" syfrah fabric --help 2>&1 || true)
for subcmd in init join peers status stop start leave peering token; do
    if echo "$output2" | grep -qi "$subcmd"; then
        pass "syfrah fabric --help: lists $subcmd"
    else
        fail "syfrah fabric --help: missing $subcmd"
    fi
done

# Test 3: syfrah --version outputs semver, not empty
info "Testing: syfrah --version..."
output3=$(docker exec "e2e-ux-help-1" syfrah --version 2>&1 || true)
if [ -n "$output3" ]; then
    pass "syfrah --version: not empty"
else
    fail "syfrah --version: empty output"
fi
if echo "$output3" | grep -qE "[0-9]+\.[0-9]+\.[0-9]+"; then
    pass "syfrah --version: contains semver"
else
    fail "syfrah --version: no semver found: $output3"
fi

# Test 4: syfrah state --help lists list, get, drop
info "Testing: syfrah state --help..."
output4=$(docker exec "e2e-ux-help-1" syfrah state --help 2>&1 || true)
for subcmd in list get drop; do
    if echo "$output4" | grep -qi "$subcmd"; then
        pass "syfrah state --help: lists $subcmd"
    else
        fail "syfrah state --help: missing $subcmd"
    fi
done

cleanup
summary
