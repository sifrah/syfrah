#!/usr/bin/env bash
# Scenario: redb/JSON consistency after concurrent joins

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── redb/JSON Consistency ──"
create_network

start_node "e2e-consist-1" "172.20.0.10"
start_node "e2e-consist-2" "172.20.0.11"
start_node "e2e-consist-3" "172.20.0.12"

init_mesh "e2e-consist-1" "172.20.0.10" "node-1"
start_peering "e2e-consist-1"

# Join 2 nodes rapidly (no delay)
docker exec -d "e2e-consist-2" syfrah fabric join 172.20.0.10:51821 \
    --node-name "node-2" --endpoint "172.20.0.11:51820" --pin "$E2E_PIN"
docker exec -d "e2e-consist-3" syfrah fabric join 172.20.0.10:51821 \
    --node-name "node-3" --endpoint "172.20.0.12:51820" --pin "$E2E_PIN"

wait_daemon "e2e-consist-2" 30
wait_daemon "e2e-consist-3" 30
wait_for_convergence "e2e-consist-" 3 2 30 || true
# Wait for JSON export to flush (debounced at 2s, retry to handle timing)
consistent=false
for attempt in $(seq 1 3); do
    sleep 2
    redb_count=$(docker exec "e2e-consist-1" syfrah state get fabric peers 2>&1 | grep -c "wg_public_key" || echo "0")
    json_count=$(docker exec "e2e-consist-1" cat /root/.syfrah/state.json 2>/dev/null | jq '.peers | length' 2>/dev/null || echo "0")
    debug "attempt $attempt: redb=$redb_count json=$json_count"
    if [ "$redb_count" = "$json_count" ]; then
        consistent=true
        break
    fi
done

info "redb peers: $redb_count, JSON peers: $json_count"

if [ "$consistent" = true ]; then
    pass "redb ($redb_count) and JSON ($json_count) peer counts match"
else
    fail "redb ($redb_count) and JSON ($json_count) peer counts diverge"
fi

# Also verify JSON is valid
if docker exec "e2e-consist-1" cat /root/.syfrah/state.json 2>/dev/null | jq . >/dev/null 2>&1; then
    pass "state.json is valid JSON"
else
    fail "state.json is invalid JSON"
fi

cleanup
summary
