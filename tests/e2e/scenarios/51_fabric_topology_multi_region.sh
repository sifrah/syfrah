#!/usr/bin/env bash
# Scenario: 2 regions, cross-region peering + topology display

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: Multi-Region ──"

create_network

start_node "e2e-tmr-1" "172.20.0.10"
start_node "e2e-tmr-2" "172.20.0.11"
start_node "e2e-tmr-3" "172.20.0.12"

# Init leader in eu-west
docker exec -d "e2e-tmr-1" \
    syfrah fabric init \
    --name mr-mesh \
    --node-name node-eu-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-1a

wait_daemon "e2e-tmr-1"
start_peering "e2e-tmr-1"

# Join node-2 in us-east region
docker exec -d "e2e-tmr-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-us-1 \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN" \
    --region us-east \
    --zone us-east-1a

wait_daemon "e2e-tmr-2"

# Join node-3 also in eu-west but different zone
docker exec -d "e2e-tmr-3" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-eu-2 \
    --endpoint 172.20.0.12:51820 \
    --pin "$E2E_PIN" \
    --region eu-west \
    --zone eu-west-1b

wait_daemon "e2e-tmr-3"

sleep 3

# Topology from leader should show both regions
topo=$(docker exec "e2e-tmr-1" syfrah fabric topology 2>&1)

if echo "$topo" | grep -q "eu-west"; then
    pass "topology shows eu-west region"
else
    fail "topology missing eu-west"
    echo "$topo"
fi

if echo "$topo" | grep -q "us-east"; then
    pass "topology shows us-east region"
else
    fail "topology missing us-east"
    echo "$topo"
fi

# Should show 2 regions in header
if echo "$topo" | grep -q "Regions:.*2"; then
    pass "topology header shows 2 regions"
else
    fail "topology header missing region count"
    echo "$topo"
fi

# Should show 3 zones total (eu-west-1a, eu-west-1b, us-east-1a)
if echo "$topo" | grep -q "eu-west-1a"; then
    pass "topology shows zone eu-west-1a"
else
    fail "topology missing zone eu-west-1a"
    echo "$topo"
fi

if echo "$topo" | grep -q "eu-west-1b"; then
    pass "topology shows zone eu-west-1b"
else
    fail "topology missing zone eu-west-1b"
    echo "$topo"
fi

if echo "$topo" | grep -q "us-east-1a"; then
    pass "topology shows zone us-east-1a"
else
    fail "topology missing zone us-east-1a"
    echo "$topo"
fi

# Verify cross-region nodes are visible
if echo "$topo" | grep -q "node-us-1"; then
    pass "eu-west leader sees us-east peer in topology"
else
    fail "cross-region peer missing from topology"
    echo "$topo"
fi

# Check from the us-east node too
topo_us=$(docker exec "e2e-tmr-2" syfrah fabric topology 2>&1)

if echo "$topo_us" | grep -q "eu-west"; then
    pass "us-east node sees eu-west in topology"
else
    fail "us-east node missing eu-west in topology"
    echo "$topo_us"
fi

cleanup
summary
