#!/usr/bin/env bash
# Scenario: UX Errors — init error messages
# Validates init produces helpful errors in every failure mode.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Errors: Init ──"
trap cleanup EXIT
create_network

start_node "e2e-err-init-1" "172.20.0.10"

# Test 1: Init when mesh already exists
info "Testing: init when mesh already exists..."
init_mesh "e2e-err-init-1" "172.20.0.10" "err-node-1"

output=$(docker exec "e2e-err-init-1" syfrah fabric init \
    --name another-mesh --node-name err-node-2 --endpoint 172.20.0.10:51820 2>&1 || true)

if echo "$output" | grep -qi "already.*exist\|already.*init"; then
    pass "double init: says already exists"
else
    fail "double init: unclear message: $(echo "$output" | head -3)"
fi

if echo "$output" | grep -qF "syfrah fabric leave"; then
    pass "double init: suggests 'syfrah fabric leave'"
else
    fail "double init: doesn't suggest leave command"
fi

# Verify no raw errors
if echo "$output" | grep -qEi "os error|anyhow|panicked|unwrap"; then
    fail "double init: contains raw error"
else
    pass "double init: no raw errors"
fi

cleanup
summary
