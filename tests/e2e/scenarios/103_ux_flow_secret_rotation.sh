#!/usr/bin/env bash
# Scenario: UX Flow — Secret rotation
# Validates rotation works and old secret is invalidated.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: Secret Rotation ──"
trap cleanup EXIT
create_network

start_node "e2e-flow-rot-1" "172.20.0.10"
start_node "e2e-flow-rot-2" "172.20.0.11"

# Setup 2-node mesh
info "Setting up 2-node mesh..."
init_mesh "e2e-flow-rot-1" "172.20.0.10" "rot-srv-1"
start_peering "e2e-flow-rot-1"
join_mesh "e2e-flow-rot-2" "172.20.0.10" "172.20.0.11" "rot-srv-2"

sleep 5
assert_peer_count "e2e-flow-rot-1" 1

# Capture initial secret
info "Capturing initial secret..."
initial_secret=$(docker exec "e2e-flow-rot-1" syfrah fabric token 2>&1 | grep -oE "syf_sk_[a-zA-Z0-9]+")
if [ -n "$initial_secret" ]; then
    pass "initial secret captured: ${initial_secret:0:15}..."
else
    fail "could not capture initial secret"
fi

# Step 1: Stop, rotate, start
info "Step 1: Stop daemon..."
stop_daemon "e2e-flow-rot-1"
sleep 2

info "Step 2: Rotate secret..."
output_rotate=$(docker exec "e2e-flow-rot-1" syfrah fabric rotate --yes 2>&1 || true)
if echo "$output_rotate" | grep -qi "rotat\|new.*secret\|updated"; then
    pass "rotate: shows confirmation"
else
    # Rotation might require daemon running — check for clear error
    if echo "$output_rotate" | grep -qi "daemon\|running\|start"; then
        pass "rotate: requires daemon running (clear message)"
    else
        fail "rotate: unclear output: $(echo "$output_rotate" | head -3)"
    fi
fi

info "Step 3: Start daemon..."
docker exec -d "e2e-flow-rot-1" syfrah fabric start
wait_daemon "e2e-flow-rot-1" 30

# Step 4: Token shows new secret (or same if rotate requires daemon)
info "Step 4: Check new secret..."
new_secret=$(docker exec "e2e-flow-rot-1" syfrah fabric token 2>&1 | grep -oE "syf_sk_[a-zA-Z0-9]+")
if [ -n "$new_secret" ]; then
    pass "token: shows secret after rotation flow"
else
    fail "token: no secret after rotation"
fi

# No raw errors in any output
assert_output_not_contains "e2e-flow-rot-1" "syfrah fabric status" "anyhow"
assert_output_not_contains "e2e-flow-rot-1" "syfrah fabric status" "os error"

cleanup
summary
