#!/usr/bin/env bash
# Scenario: old state.json (without topology field) loads correctly

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Topology: Backward Compatibility ──"

create_network

start_node "e2e-tcompat-1" "172.20.0.10"
start_node "e2e-tcompat-2" "172.20.0.11"

# Init without explicit region/zone (simulates legacy behavior)
init_mesh "e2e-tcompat-1" "172.20.0.10" "node-1"
start_peering "e2e-tcompat-1"
join_mesh "e2e-tcompat-2" "172.20.0.10" "172.20.0.11" "node-2"

wait_for_convergence "e2e-tcompat-" 2 1 30 || true

# Topology should still work with default region/zone
topo=$(docker exec "e2e-tcompat-1" syfrah fabric topology 2>&1)

if echo "$topo" | grep -q "default"; then
    pass "topology shows default region for legacy nodes"
else
    fail "topology fails with nodes that have no explicit region"
    echo "$topo"
fi

if echo "$topo" | grep -q "node-1"; then
    pass "legacy node-1 visible in topology"
else
    fail "legacy node-1 missing from topology"
    echo "$topo"
fi

if echo "$topo" | grep -q "node-2"; then
    pass "legacy node-2 visible in topology"
else
    fail "legacy node-2 missing from topology"
    echo "$topo"
fi

# JSON output should also work for legacy nodes
json=$(docker exec "e2e-tcompat-1" syfrah fabric topology --json 2>&1)

if echo "$json" | jq . >/dev/null 2>&1; then
    pass "topology --json works for legacy nodes"
else
    fail "topology --json fails for legacy nodes"
    echo "$json"
fi

# Default region should appear in JSON
default_region=$(echo "$json" | jq -r '.regions[].name' | grep "default")
if [ -n "$default_region" ]; then
    pass "JSON shows default region for legacy nodes"
else
    fail "JSON missing default region"
    echo "$json"
fi

# Peers command should still work
peers=$(docker exec "e2e-tcompat-1" syfrah fabric peers 2>&1)

if echo "$peers" | grep -q "node-2"; then
    pass "peers command works alongside topology for legacy state"
else
    fail "peers command broken for legacy state"
    echo "$peers"
fi

# Now inject a minimal legacy state.json without topology field
# and verify it still loads
docker exec "e2e-tcompat-1" syfrah fabric stop 2>&1 || true
sleep 1

# Read the existing state.json and strip the topology field
docker exec "e2e-tcompat-1" sh -c '
    if [ -f /root/.syfrah/state.json ]; then
        cat /root/.syfrah/state.json | \
            jq "del(.peers[].topology)" > /tmp/legacy_state.json && \
            cp /tmp/legacy_state.json /root/.syfrah/state.json
    fi
'

# Restart and verify topology still works
docker exec -d "e2e-tcompat-1" \
    syfrah fabric start

wait_daemon "e2e-tcompat-1" || true

sleep 2

topo_after=$(docker exec "e2e-tcompat-1" syfrah fabric topology 2>&1)

if echo "$topo_after" | grep -q "default\|node-1\|Topology"; then
    pass "topology loads after stripping topology field from state"
else
    fail "topology broken after stripping topology field"
    echo "$topo_after"
fi

cleanup
summary
