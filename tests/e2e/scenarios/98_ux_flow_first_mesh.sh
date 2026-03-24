#!/usr/bin/env bash
# Scenario: UX Flow — First-time mesh setup
# Validates the complete onboarding in the fewest possible steps.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Flow: First Mesh ──"
trap cleanup EXIT
create_network

start_node "e2e-flow-first-1" "172.20.0.10"
start_node "e2e-flow-first-2" "172.20.0.11"

# Step 1: Server 1 init
info "Step 1: Server 1 creates mesh..."
output_init=$(docker exec "e2e-flow-first-1" syfrah fabric init \
    --name first-mesh --node-name server-1 --endpoint 172.20.0.10:51820 2>&1)

echo "$output_init" | grep -qF "syf_sk_" || fail "init: no secret shown"
pass "init: secret displayed"

wait_daemon "e2e-flow-first-1" 30

# Step 2: Server 1 starts peering with PIN
info "Step 2: Server 1 starts peering..."
docker exec "e2e-flow-first-1" syfrah fabric peering start --pin "$E2E_PIN"
sleep 2

# Step 3: Server 2 joins with PIN
info "Step 3: Server 2 joins with PIN..."
output_join=$(docker exec "e2e-flow-first-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name server-2 --endpoint 172.20.0.11:51820 --pin "$E2E_PIN" 2>&1)

echo "$output_join" | grep -qi "joined\|approved\|accepted" || fail "join: no approval shown"
pass "join: approval confirmed"

wait_daemon "e2e-flow-first-2" 30
sleep 5

# Step 4: Both see each other in peers
info "Step 4: Verify bidirectional peer visibility..."
output_peers1=$(docker exec "e2e-flow-first-1" syfrah fabric peers 2>&1)
echo "$output_peers1" | grep -q "server-2" || fail "server-1 doesn't see server-2"
pass "server-1 sees server-2 in peers"

output_peers2=$(docker exec "e2e-flow-first-2" syfrah fabric peers 2>&1)
echo "$output_peers2" | grep -q "server-1" || fail "server-2 doesn't see server-1"
pass "server-2 sees server-1 in peers"

# Step 5: Region/zone displayed for peers
info "Step 5: Region/zone displayed..."
assert_regions_displayed "e2e-flow-first-1"
assert_regions_displayed "e2e-flow-first-2"

# Step 6: Mesh IPv6 ping works
info "Step 6: Mesh connectivity via IPv6..."
ipv6_1=$(get_mesh_ipv6 "e2e-flow-first-1")
ipv6_2=$(get_mesh_ipv6 "e2e-flow-first-2")

if [ -n "$ipv6_1" ] && [ -n "$ipv6_2" ]; then
    assert_can_ping "e2e-flow-first-1" "$ipv6_2"
    assert_can_ping "e2e-flow-first-2" "$ipv6_1"
else
    fail "could not get mesh IPv6 addresses"
fi

# Final: No duplicates, no epoch dates
for node in "e2e-flow-first-1" "e2e-flow-first-2"; do
    assert_no_duplicate_peers "$node"
    assert_no_epoch_dates "$node"
done

cleanup
summary
