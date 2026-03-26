#!/usr/bin/env bash
# Scenario: topology --json output schema validation

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: JSON Output ──"

create_network

start_node "e2e-tjson-1" "172.20.0.10"
start_node "e2e-tjson-2" "172.20.0.11"

# Init with known region/zone
docker exec -d "e2e-tjson-1" \
    syfrah fabric init \
    --name json-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region eu-west \
    --zone eu-west-1a

wait_daemon "e2e-tjson-1"
start_peering "e2e-tjson-1"

join_mesh "e2e-tjson-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

# Get JSON output
json=$(docker exec "e2e-tjson-1" syfrah fabric topology --json 2>&1)

# Should be valid JSON
if echo "$json" | jq . >/dev/null 2>&1; then
    pass "topology --json produces valid JSON"
else
    fail "topology --json is not valid JSON"
    echo "$json"
fi

# Check top-level fields
if echo "$json" | jq -e '.mesh_name' >/dev/null 2>&1; then
    pass "JSON has mesh_name field"
else
    fail "JSON missing mesh_name field"
fi

if echo "$json" | jq -e '.total_nodes' >/dev/null 2>&1; then
    pass "JSON has total_nodes field"
else
    fail "JSON missing total_nodes field"
fi

if echo "$json" | jq -e '.regions' >/dev/null 2>&1; then
    pass "JSON has regions array"
else
    fail "JSON missing regions array"
fi

# Check mesh_name value
mesh_name=$(echo "$json" | jq -r '.mesh_name')
if [ "$mesh_name" = "json-mesh" ]; then
    pass "mesh_name is correct: json-mesh"
else
    fail "mesh_name incorrect: $mesh_name"
fi

# Check total_nodes value (leader + joiner = 2 peers visible)
total_nodes=$(echo "$json" | jq -r '.total_nodes')
if [ "$total_nodes" -ge 2 ] 2>/dev/null; then
    pass "total_nodes >= 2"
else
    fail "total_nodes unexpected: $total_nodes"
fi

# Check regions array has items
region_count=$(echo "$json" | jq '.regions | length')
if [ "$region_count" -ge 1 ] 2>/dev/null; then
    pass "regions array has $region_count entries"
else
    fail "regions array empty"
fi

# Check region structure: name and zones
first_region_name=$(echo "$json" | jq -r '.regions[0].name')
if [ -n "$first_region_name" ] && [ "$first_region_name" != "null" ]; then
    pass "region has name field: $first_region_name"
else
    fail "region missing name field"
fi

first_region_zones=$(echo "$json" | jq '.regions[0].zones | length')
if [ "$first_region_zones" -ge 1 ] 2>/dev/null; then
    pass "region has zones array with $first_region_zones entries"
else
    fail "region zones array empty"
fi

# Check zone structure: name and nodes
first_zone_name=$(echo "$json" | jq -r '.regions[0].zones[0].name')
if [ -n "$first_zone_name" ] && [ "$first_zone_name" != "null" ]; then
    pass "zone has name field: $first_zone_name"
else
    fail "zone missing name field"
fi

first_zone_nodes=$(echo "$json" | jq '.regions[0].zones[0].nodes | length')
if [ "$first_zone_nodes" -ge 1 ] 2>/dev/null; then
    pass "zone has nodes array with $first_zone_nodes entries"
else
    fail "zone nodes array empty"
fi

# Check node structure: name, mesh_ipv6, status
node_name=$(echo "$json" | jq -r '.regions[0].zones[0].nodes[0].name')
if [ -n "$node_name" ] && [ "$node_name" != "null" ]; then
    pass "node has name field: $node_name"
else
    fail "node missing name field"
fi

node_ipv6=$(echo "$json" | jq -r '.regions[0].zones[0].nodes[0].mesh_ipv6')
if [ -n "$node_ipv6" ] && [ "$node_ipv6" != "null" ]; then
    pass "node has mesh_ipv6 field"
else
    fail "node missing mesh_ipv6 field"
fi

node_status=$(echo "$json" | jq -r '.regions[0].zones[0].nodes[0].status')
if [ "$node_status" = "active" ] || [ "$node_status" = "unreachable" ]; then
    pass "node has valid status: $node_status"
else
    fail "node has unexpected status: $node_status"
fi

# JSON with --region filter should still be valid JSON
filtered_json=$(docker exec "e2e-tjson-1" syfrah fabric topology --json --region eu-west 2>&1)
if echo "$filtered_json" | jq . >/dev/null 2>&1; then
    pass "topology --json --region produces valid JSON"
else
    fail "topology --json --region is not valid JSON"
    echo "$filtered_json"
fi

cleanup
summary
