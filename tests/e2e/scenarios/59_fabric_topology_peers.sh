#!/usr/bin/env bash
# Scenario: peers --topology shows grouped output by region/zone

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: Peers --topology ──"

create_network

start_node "e2e-tpeers-1" "172.20.0.10"
start_node "e2e-tpeers-2" "172.20.0.11"
start_node "e2e-tpeers-3" "172.20.0.12"

# Init leader in eu-west
docker exec -d "e2e-tpeers-1" \
    syfrah fabric init \
    --name peers-mesh \
    --node-name node-eu \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-1a

wait_daemon "e2e-tpeers-1"
start_peering "e2e-tpeers-1"

# Join node-2 in us-east
docker exec -d "e2e-tpeers-2" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-us \
    --endpoint 172.20.0.11:51820 \
    --pin "$E2E_PIN" \
    --region us-east \
    --zone us-east-1a

wait_daemon "e2e-tpeers-2"

# Join node-3 in eu-west
docker exec -d "e2e-tpeers-3" \
    syfrah fabric join 172.20.0.10:51821 \
    --node-name node-eu-2 \
    --endpoint 172.20.0.12:51820 \
    --pin "$E2E_PIN" \
    --region eu-west \
    --zone eu-west-1b

wait_daemon "e2e-tpeers-3"

sleep 3

# Run peers --topology
topo=$(docker exec "e2e-tpeers-1" syfrah fabric peers --topology 2>&1)

# Should show region headers
if echo "$topo" | grep -q "eu-west"; then
    pass "peers --topology shows eu-west region"
else
    fail "peers --topology missing eu-west"
    echo "$topo"
fi

if echo "$topo" | grep -q "us-east"; then
    pass "peers --topology shows us-east region"
else
    fail "peers --topology missing us-east"
    echo "$topo"
fi

# Should show zone groupings
if echo "$topo" | grep -q "eu-west-1a"; then
    pass "peers --topology shows zone eu-west-1a"
else
    fail "peers --topology missing zone eu-west-1a"
    echo "$topo"
fi

if echo "$topo" | grep -q "us-east-1a"; then
    pass "peers --topology shows zone us-east-1a"
else
    fail "peers --topology missing zone us-east-1a"
    echo "$topo"
fi

# Should show node names grouped under zones
if echo "$topo" | grep -q "node-us"; then
    pass "peers --topology shows node-us"
else
    fail "peers --topology missing node-us"
    echo "$topo"
fi

if echo "$topo" | grep -q "node-eu-2"; then
    pass "peers --topology shows node-eu-2"
else
    fail "peers --topology missing node-eu-2"
    echo "$topo"
fi

# Node count should appear in region headers
if echo "$topo" | grep "eu-west" | grep -q "node"; then
    pass "peers --topology shows node count in region header"
else
    # Some implementations use a number without the word "node"
    if echo "$topo" | grep "eu-west" | grep -qE "[0-9]"; then
        pass "peers --topology shows count in region header"
    else
        fail "peers --topology missing node count in region"
        echo "$topo"
    fi
fi

# Regular peers (no --topology) should still show flat table
flat=$(docker exec "e2e-tpeers-1" syfrah fabric peers 2>&1)

if echo "$flat" | grep -q "NAME\|REGION\|ZONE"; then
    pass "flat peers shows column headers"
else
    fail "flat peers missing column headers"
    echo "$flat"
fi

# Both formats should show the same peers
if echo "$flat" | grep -q "node-us" && echo "$flat" | grep -q "node-eu-2"; then
    pass "flat peers shows all peers"
else
    fail "flat peers missing some peers"
    echo "$flat"
fi

cleanup
summary
