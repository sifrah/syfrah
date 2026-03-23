#!/usr/bin/env bash
# Scenario: manual --region and --zone override the defaults

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Manual Override ──"

create_network

start_node "e2e-zman-1" "${E2E_IP_PREFIX}.10"
start_node "e2e-zman-2" "${E2E_IP_PREFIX}.11"

# Init with custom region/zone
docker exec -d "e2e-zman-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint ${E2E_IP_PREFIX}.10:51820 \
    --region eu-west \
    --zone eu-west-paris-1

wait_daemon "e2e-zman-1"

status1=$(docker exec "e2e-zman-1" syfrah fabric status 2>&1)

if echo "$status1" | grep -q "Region:.*eu-west"; then
    pass "node-1 has manual region: eu-west"
else
    fail "node-1 region override failed"
    echo "$status1"
fi

if echo "$status1" | grep -q "Zone:.*eu-west-paris-1"; then
    pass "node-1 has manual zone: eu-west-paris-1"
else
    fail "node-1 zone override failed"
    echo "$status1"
fi

# Join with custom region/zone
start_peering "e2e-zman-1"
docker exec -d "e2e-zman-2" \
    syfrah fabric join ${E2E_IP_PREFIX}.10:51821 \
    --node-name node-2 \
    --endpoint ${E2E_IP_PREFIX}.11:51820 \
    --pin "$E2E_PIN" \
    --region eu-central \
    --zone eu-central-frankfurt-1

wait_daemon "e2e-zman-2"

status2=$(docker exec "e2e-zman-2" syfrah fabric status 2>&1)

if echo "$status2" | grep -q "Region:.*eu-central"; then
    pass "node-2 has manual region: eu-central"
else
    fail "node-2 region override failed"
    echo "$status2"
fi

if echo "$status2" | grep -q "Zone:.*eu-central-frankfurt-1"; then
    pass "node-2 has manual zone: eu-central-frankfurt-1"
else
    fail "node-2 zone override failed"
    echo "$status2"
fi

cleanup
summary
