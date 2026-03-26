#!/usr/bin/env bash
# Scenario: topology command shows tree after 3-node setup

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: Basic Tree ──"

create_network

start_node "e2e-topo-1" "172.20.0.10"
start_node "e2e-topo-2" "172.20.0.11"
start_node "e2e-topo-3" "172.20.0.12"

# Init leader with explicit region/zone
docker exec -d "e2e-topo-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-1a

wait_daemon "e2e-topo-1"
start_peering "e2e-topo-1"

join_mesh "e2e-topo-2" "172.20.0.10" "172.20.0.11" "node-2"
join_mesh "e2e-topo-3" "172.20.0.10" "172.20.0.12" "node-3"

sleep 3

# Run topology command on the leader
topo=$(docker exec "e2e-topo-1" syfrah fabric topology 2>&1)

# Should display the mesh name
if echo "$topo" | grep -q "test-mesh"; then
    pass "topology shows mesh name"
else
    fail "topology missing mesh name"
    echo "$topo"
fi

# Should show region header
if echo "$topo" | grep -q "eu-west"; then
    pass "topology shows region eu-west"
else
    fail "topology missing region eu-west"
    echo "$topo"
fi

# Should show zone under region
if echo "$topo" | grep -q "eu-west-1a"; then
    pass "topology shows zone eu-west-1a"
else
    fail "topology missing zone eu-west-1a"
    echo "$topo"
fi

# Should list node-1 (leader)
if echo "$topo" | grep -q "node-1"; then
    pass "topology shows node-1"
else
    fail "topology missing node-1"
    echo "$topo"
fi

# Should show the joiners (they get default region/zone)
if echo "$topo" | grep -q "node-2"; then
    pass "topology shows node-2"
else
    fail "topology missing node-2"
    echo "$topo"
fi

if echo "$topo" | grep -q "node-3"; then
    pass "topology shows node-3"
else
    fail "topology missing node-3"
    echo "$topo"
fi

# Should show node count in header
if echo "$topo" | grep -q "Nodes:.*3"; then
    pass "topology header shows 3 nodes"
else
    fail "topology header missing node count"
    echo "$topo"
fi

# The default region should also appear (for joiners)
if echo "$topo" | grep -q "default"; then
    pass "topology shows default region for joiners"
else
    fail "topology missing default region"
    echo "$topo"
fi

cleanup
summary
