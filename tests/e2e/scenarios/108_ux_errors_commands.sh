#!/usr/bin/env bash
# Scenario: UX Errors — wrong command paths and suggestions
# Validates typos and wrong paths get helpful responses.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Errors: Commands ──"
trap cleanup EXIT
create_network

start_node "e2e-err-cmd-1" "172.20.0.10"

# Test 1: syfrah without subcommand — shows help
info "Testing: syfrah without subcommand..."
err=$(docker exec "e2e-err-cmd-1" syfrah 2>&1 || true)
if echo "$err" | grep -qi "help\|usage\|fabric\|commands"; then
    pass "syfrah bare: shows help/commands"
else
    fail "syfrah bare: no help: $(echo "$err" | head -3)"
fi

# Test 2: syfrah with wrong subcommand — helpful error
info "Testing: syfrah with wrong subcommand..."
err2=$(docker exec "e2e-err-cmd-1" syfrah peering 2>&1 || true)
if echo "$err2" | grep -qi "help\|usage\|unrecognized\|invalid\|fabric\|not found\|subcommand"; then
    pass "syfrah peering: shows help or error"
else
    fail "syfrah peering: no guidance: $(echo "$err2" | head -3)"
fi

err3=$(docker exec "e2e-err-cmd-1" syfrah init 2>&1 || true)
if echo "$err3" | grep -qi "help\|usage\|unrecognized\|invalid\|fabric\|not found\|subcommand"; then
    pass "syfrah init: shows help or error"
else
    fail "syfrah init: no guidance: $(echo "$err3" | head -3)"
fi

err4=$(docker exec "e2e-err-cmd-1" syfrah join 2>&1 || true)
if echo "$err4" | grep -qi "help\|usage\|unrecognized\|invalid\|fabric\|not found\|subcommand"; then
    pass "syfrah join: shows help or error"
else
    fail "syfrah join: no guidance: $(echo "$err4" | head -3)"
fi

# Test 3: All suggested commands in error messages are valid
info "Testing: suggested commands in error output are valid..."
# Init to get a mesh, then test commands
init_mesh "e2e-err-cmd-1" "172.20.0.10" "cmd-node-1"
assert_all_commands_valid "e2e-err-cmd-1" "syfrah fabric status"

cleanup
summary
