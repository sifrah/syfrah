#!/usr/bin/env bash
# Scenario: --region and --zone filters on topology command

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: Region/Zone Filters ──"

create_network

start_node "e2e-tfilt-1" "172.20.0.10"
start_node "e2e-tfilt-2" "172.20.0.11"
start_node "e2e-tfilt-3" "172.20.0.12"

# Init leader in eu-west / eu-west-1a
docker exec -d "e2e-tfilt-1" \
    syfrah fabric init \
    --name filter-mesh \
    --node-name node-eu \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-1a

wait_daemon "e2e-tfilt-1"
start_peering "e2e-tfilt-1"

# Join node-2 in us-east
docker exec -d "e2e-tfilt-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-us \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN" \
    --region us-east \
    --zone us-east-1a

wait_daemon "e2e-tfilt-2"

# Join node-3 in eu-west / eu-west-1b
docker exec -d "e2e-tfilt-3" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-eu-2 \
    --endpoint 172.20.0.12:51820 \
    --pin "$E2E_PIN" \
    --region eu-west \
    --zone eu-west-1b

wait_daemon "e2e-tfilt-3"

sleep 3

# Filter by --region eu-west: should show eu-west, NOT us-east
filtered=$(docker exec "e2e-tfilt-1" syfrah fabric topology --region eu-west 2>&1)

if echo "$filtered" | grep -q "eu-west"; then
    pass "region filter shows eu-west"
else
    fail "region filter missing eu-west"
    echo "$filtered"
fi

if echo "$filtered" | grep -q "us-east"; then
    fail "region filter should hide us-east"
    echo "$filtered"
else
    pass "region filter hides us-east"
fi

if echo "$filtered" | grep -q "node-eu"; then
    pass "region filter shows node-eu"
else
    fail "region filter missing node-eu"
    echo "$filtered"
fi

# Filter by --zone eu-west-1a: should show only that zone's nodes
zone_filtered=$(docker exec "e2e-tfilt-1" syfrah fabric topology --zone eu-west-1a 2>&1)

if echo "$zone_filtered" | grep -q "eu-west-1a"; then
    pass "zone filter shows eu-west-1a"
else
    fail "zone filter missing eu-west-1a"
    echo "$zone_filtered"
fi

if echo "$zone_filtered" | grep -q "node-eu \\|node-eu$"; then
    pass "zone filter shows node-eu in eu-west-1a"
else
    # The leader node-eu is in eu-west-1a
    if echo "$zone_filtered" | grep -q "node-eu"; then
        pass "zone filter shows node-eu"
    else
        fail "zone filter missing node-eu"
        echo "$zone_filtered"
    fi
fi

# Invalid region filter should fail with helpful message
invalid=$(docker exec "e2e-tfilt-1" syfrah fabric topology --region nonexistent 2>&1 || true)

if echo "$invalid" | grep -qi "no region\|available\|not found\|error"; then
    pass "invalid region filter gives helpful error"
else
    fail "invalid region filter: unhelpful message"
    echo "$invalid"
fi

# Invalid zone filter should fail with helpful message
invalid_zone=$(docker exec "e2e-tfilt-1" syfrah fabric topology --zone nonexistent 2>&1 || true)

if echo "$invalid_zone" | grep -qi "no zone\|available\|not found\|error"; then
    pass "invalid zone filter gives helpful error"
else
    fail "invalid zone filter: unhelpful message"
    echo "$invalid_zone"
fi

cleanup
summary
