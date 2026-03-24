#!/usr/bin/env bash
# Scenario: UX Errors — join error messages
# Validates join produces helpful errors in every failure mode.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Errors: Join ──"
trap cleanup EXIT
create_network

start_node "e2e-err-join-1" "172.20.0.10"
start_node "e2e-err-join-2" "172.20.0.11"
start_node "e2e-err-join-3" "172.20.0.12"

# Set up mesh on node 1 for some tests
init_mesh "e2e-err-join-1" "172.20.0.10" "err-join-srv-1"

# Test 1: Join to unreachable IP — human message
info "Testing: join to unreachable IP..."
err=$(docker exec "e2e-err-join-2" syfrah fabric join 172.20.0.99:51821 \
    --node-name err-join-2 --endpoint 172.20.0.11:51820 --pin 1234 2>&1 || true)
if echo "$err" | grep -qEi "os error 111|ECONNREFUSED"; then
    fail "join unreachable: raw OS error visible: $(echo "$err" | head -3)"
else
    pass "join unreachable: no raw OS error"
fi

# Test 2: Join when peering not active — human message
info "Testing: join when peering not active..."
err2=$(docker exec "e2e-err-join-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name err-join-2 --endpoint 172.20.0.11:51820 --pin 1234 2>&1 || true)
if echo "$err2" | grep -qEi "early eof"; then
    fail "join no peering: raw 'early eof' visible"
else
    pass "join no peering: no raw error"
fi

# Test 3: Join when state exists — suggests syfrah fabric leave
info "Testing: join when state exists..."
start_peering "e2e-err-join-1"
join_mesh "e2e-err-join-2" "172.20.0.10" "172.20.0.11" "err-join-srv-2"
sleep 3

err3=$(docker exec "e2e-err-join-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name err-join-2b --endpoint 172.20.0.11:51820 --pin "$E2E_PIN" 2>&1 || true)
if echo "$err3" | grep -qi "already\|exists\|leave"; then
    pass "join with state: mentions existing state"
else
    fail "join with state: unclear: $(echo "$err3" | head -3)"
fi

if echo "$err3" | grep -qF "syfrah fabric leave"; then
    pass "join with state: suggests full command path"
elif echo "$err3" | grep -qi "leave"; then
    pass "join with state: mentions leave"
else
    fail "join with state: no leave suggestion"
fi

# Test 4: Join no arguments — shows help
info "Testing: join no arguments..."
err4=$(docker exec "e2e-err-join-3" syfrah fabric join 2>&1 || true)
if echo "$err4" | grep -qi "usage\|help\|argument\|required"; then
    pass "join no args: shows usage"
else
    fail "join no args: no usage info: $(echo "$err4" | head -3)"
fi

# Test 5: Join after failed join — retry works (no phantom state)
info "Testing: join retry after failure..."
assert_join_retry_works "e2e-err-join-3" "172.20.0.10:51821" "172.20.0.12" "err-join-3"

cleanup
summary
