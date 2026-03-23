#!/usr/bin/env bash
# Scenario: syfrah fabric peers displays region/zone columns

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Peers Display ──"

create_network

start_node "e2e-zdisp-1" "172.20.0.10"
start_node "e2e-zdisp-2" "172.20.0.11"

# Init with known region/zone
docker exec -d "e2e-zdisp-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-zone-1

wait_daemon "e2e-zdisp-1"
start_peering "e2e-zdisp-1"

docker exec -d "e2e-zdisp-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-2 \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN" \
    --region eu-west \
    --zone eu-west-zone-2

wait_daemon "e2e-zdisp-2"

sleep 3

# Check peers output from node-1
output=$(docker exec "e2e-zdisp-1" syfrah fabric peers 2>&1)

if echo "$output" | grep -q "eu-west"; then
    pass "peers output shows region eu-west"
else
    fail "peers output missing region"
    echo "$output"
fi

if echo "$output" | grep -q "eu-west-zone-2"; then
    pass "peers output shows zone eu-west-zone-2"
else
    fail "peers output missing zone"
    echo "$output"
fi

cleanup
summary
