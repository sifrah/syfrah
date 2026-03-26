#!/usr/bin/env bash
# Scenario: invalid region/zone names are rejected by init and join

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: Name Validation ──"

create_network

start_node "e2e-tval-1" "172.20.0.10"
start_node "e2e-tval-2" "172.20.0.11"

# Uppercase region should be rejected
info "Testing: uppercase region..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region EU-WEST \
    --zone eu-west-1a 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "uppercase region rejected"
else
    fail "uppercase region not rejected: $err"
fi

# Leading dash in region should be rejected
info "Testing: leading dash in region..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region "-bad-region" \
    --zone zone-1 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "leading dash region rejected"
else
    fail "leading dash region not rejected: $err"
fi

# Trailing dash in region should be rejected
info "Testing: trailing dash in region..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region "bad-region-" \
    --zone zone-1 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "trailing dash region rejected"
else
    fail "trailing dash region not rejected: $err"
fi

# Special characters in region should be rejected
info "Testing: special characters in region..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region "eu_west" \
    --zone zone-1 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "underscore in region rejected"
else
    fail "underscore in region not rejected: $err"
fi

# Empty region should be rejected
info "Testing: empty region..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region "" \
    --zone zone-1 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected\|empty\|required"; then
    pass "empty region rejected"
else
    fail "empty region not rejected: $err"
fi

# Uppercase zone should be rejected
info "Testing: uppercase zone..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone ZONE-A 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "uppercase zone rejected"
else
    fail "uppercase zone not rejected: $err"
fi

# Leading dash in zone should be rejected
info "Testing: leading dash in zone..."
err=$(docker exec "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone "-zone" 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "leading dash zone rejected"
else
    fail "leading dash zone not rejected: $err"
fi

# Valid init should succeed after rejections
info "Testing: valid region/zone accepted..."
docker exec -d "e2e-tval-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-1a

wait_daemon "e2e-tval-1"

status=$(docker exec "e2e-tval-1" syfrah fabric status 2>&1)
if echo "$status" | grep -q "eu-west"; then
    pass "valid region accepted after rejections"
else
    fail "valid init failed"
    echo "$status"
fi

# Test join with invalid region
start_peering "e2e-tval-1"

info "Testing: join with invalid region..."
err=$(docker exec "e2e-tval-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-2 \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN" \
    --region "BAD REGION" \
    --zone zone-1 2>&1 || true)

if echo "$err" | grep -qi "invalid\|error\|rejected"; then
    pass "join with invalid region rejected"
else
    fail "join with invalid region not rejected: $err"
fi

cleanup
summary
