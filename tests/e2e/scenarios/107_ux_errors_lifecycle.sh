#!/usr/bin/env bash
# Scenario: UX Errors — lifecycle command error messages
# Validates stop, start, leave, peers, status produce helpful errors.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Errors: Lifecycle ──"
trap cleanup EXIT
create_network

start_node "e2e-err-life-1" "172.20.0.10"

# Test 1: stop when not running
info "Testing: stop when not running..."
err=$(docker exec "e2e-err-life-1" syfrah fabric stop 2>&1 || true)
if echo "$err" | grep -qi "not running\|nothing\|no daemon\|already"; then
    pass "stop not running: helpful message"
else
    fail "stop not running: unclear: $(echo "$err" | head -3)"
fi

# Test 2: start without init — suggests init/join
info "Testing: start without init..."
err2=$(docker exec "e2e-err-life-1" syfrah fabric start 2>&1 || true)
if echo "$err2" | grep -qi "init\|join\|configured\|no mesh"; then
    pass "start without init: suggests init/join"
else
    fail "start without init: unclear: $(echo "$err2" | head -3)"
fi

# Test 3: leave without mesh
info "Testing: leave without mesh..."
err3=$(docker exec "e2e-err-life-1" syfrah fabric leave --yes 2>&1 || true)
if echo "$err3" | grep -qi "nothing\|no mesh\|not.*found\|no state\|already"; then
    pass "leave no mesh: says nothing to do"
else
    fail "leave no mesh: unclear: $(echo "$err3" | head -3)"
fi

# Test 4: double leave
info "Testing: double leave..."
err4=$(docker exec "e2e-err-life-1" syfrah fabric leave --yes 2>&1 || true)
if echo "$err4" | grep -qi "nothing\|no mesh\|not.*found\|no state\|already"; then
    pass "double leave: says nothing to do"
else
    fail "double leave: unclear: $(echo "$err4" | head -3)"
fi

# Test 5: peers without mesh — suggests init/join
info "Testing: peers without mesh..."
err5=$(docker exec "e2e-err-life-1" syfrah fabric peers 2>&1 || true)
if echo "$err5" | grep -qi "init\|join"; then
    pass "peers no mesh: suggests init/join"
else
    fail "peers no mesh: unclear: $(echo "$err5" | head -3)"
fi

# Test 6: status without mesh — suggests init/join
info "Testing: status without mesh..."
err6=$(docker exec "e2e-err-life-1" syfrah fabric status 2>&1 || true)
if echo "$err6" | grep -qi "init\|join"; then
    pass "status no mesh: suggests init/join"
else
    fail "status no mesh: unclear: $(echo "$err6" | head -3)"
fi

# Test 7: leave then join — works first try
info "Testing: leave then join cycle..."
init_mesh "e2e-err-life-1" "172.20.0.10" "life-node-1"
docker exec "e2e-err-life-1" syfrah fabric leave --yes 2>&1 || true
sleep 2
# Should be able to init again without issues
output_reinit=$(docker exec "e2e-err-life-1" syfrah fabric init \
    --name re-mesh --node-name life-node-1 --endpoint 172.20.0.10:51820 2>&1 || true)
if echo "$output_reinit" | grep -qi "created\|mesh\|ok"; then
    pass "leave then init: works first try"
else
    fail "leave then init: failed: $(echo "$output_reinit" | head -3)"
fi

cleanup
summary
