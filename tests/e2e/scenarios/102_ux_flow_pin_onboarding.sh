#!/usr/bin/env bash
# Scenario: UX Flow — Zero-interaction PIN onboarding
# Validates the "copy-paste" onboarding experience.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: PIN Onboarding ──"
trap cleanup EXIT
create_network

start_node "e2e-flow-pin-1" "172.20.0.10"
start_node "e2e-flow-pin-2" "172.20.0.11"

# Step 1: Server 1 creates mesh
info "Step 1: Server 1 creates mesh..."
docker exec "e2e-flow-pin-1" syfrah fabric init \
    --name pin-mesh --node-name pin-srv-1 --endpoint 172.20.0.10:51820 2>&1
wait_daemon "e2e-flow-pin-1" 30

# Step 2: Server 1 starts peering with PIN
info "Step 2: Server 1 starts peering with auto PIN..."
docker exec "e2e-flow-pin-1" syfrah fabric peering start --pin "$E2E_PIN"
sleep 2

# Step 3: Server 2 joins using PIN — no manual approval needed
info "Step 3: Server 2 joins with PIN (zero interaction)..."
output_join=$(docker exec "e2e-flow-pin-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name pin-srv-2 --endpoint 172.20.0.11:51820 --pin "$E2E_PIN" 2>&1)

# Verify join succeeded automatically
echo "$output_join" | grep -qi "joined\|approved\|accepted" || fail "PIN join: no auto-approval"
pass "PIN join: automatic approval (no interaction)"

wait_daemon "e2e-flow-pin-2" 30
wait_for_peer_active "e2e-flow-pin-1" 1 30
wait_for_peer_active "e2e-flow-pin-2" 1 30

# Step 4: Both nodes see each other
info "Step 4: Verify mesh formed..."
output_peers1=$(docker exec "e2e-flow-pin-1" syfrah fabric peers 2>&1)
echo "$output_peers1" | grep -q "pin-srv-2" || fail "server-1 doesn't see server-2"
pass "server-1 sees server-2"

output_peers2=$(docker exec "e2e-flow-pin-2" syfrah fabric peers 2>&1)
echo "$output_peers2" | grep -q "pin-srv-1" || fail "server-2 doesn't see server-1"
pass "server-2 sees server-1"

# Final checks
assert_no_duplicate_peers "e2e-flow-pin-1"
assert_no_duplicate_peers "e2e-flow-pin-2"
assert_no_epoch_dates "e2e-flow-pin-1"
assert_no_epoch_dates "e2e-flow-pin-2"

cleanup
summary
