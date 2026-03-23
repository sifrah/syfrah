#!/usr/bin/env bash
# Scenario: CLI error messages are actionable

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Error Messages ──"
create_network

start_node "e2e-errmsg-1" "172.20.0.10"

# Test 1: start without init — should suggest init/join
info "Testing: start without init..."
err=$(docker exec "e2e-errmsg-1" syfrah fabric start 2>&1 || true)
if echo "$err" | grep -qi "init\|join\|configured"; then
    pass "start without init suggests init/join"
else
    fail "start without init: unhelpful message: $err"
fi

# Test 2: double init — should suggest leave
info "Testing: double init..."
init_mesh "e2e-errmsg-1" "172.20.0.10" "node-1"
err=$(docker exec "e2e-errmsg-1" syfrah fabric init \
    --name test2 --node-name node-2 --endpoint 172.20.0.10:51820 2>&1 || true)
if echo "$err" | grep -qi "leave\|already"; then
    pass "double init suggests leave"
else
    fail "double init: unhelpful message: $err"
fi

cleanup
summary
