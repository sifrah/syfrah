#!/usr/bin/env bash
# Scenario: region/zone survives daemon restart

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: Persist Across Restart ──"

create_network
start_node "e2e-zper-1" "172.20.0.10"

# Init with custom zone
docker exec -d "e2e-zper-1" \
    syfrah fabric init \
    --name test-mesh \
    --node-name node-1 \
    --endpoint 172.20.0.10:51820 \
    --region my-region \
    --zone my-region-zone-42

wait_daemon "e2e-zper-1"

# Verify zone is set
z_before=$(docker exec "e2e-zper-1" syfrah fabric status 2>&1 | grep -i "Zone:" | awk '{print $NF}')
if [ "$z_before" = "my-region-zone-42" ]; then
    pass "zone set before restart: $z_before"
else
    fail "zone before restart: $z_before"
fi

# Kill and restart
docker exec "e2e-zper-1" pkill -f syfrah 2>/dev/null || true
sleep 2

docker exec -d "e2e-zper-1" syfrah fabric start
wait_daemon "e2e-zper-1"

# Verify zone survived restart
z_after=$(docker exec "e2e-zper-1" syfrah fabric status 2>&1 | grep -i "Zone:" | awk '{print $NF}')
if [ "$z_after" = "my-region-zone-42" ]; then
    pass "zone preserved after restart: $z_after"
else
    fail "zone after restart: $z_after (expected my-region-zone-42)"
fi

cleanup
summary
