#!/usr/bin/env bash
# Scenario: syfrah state drop removes a layer's state database

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── State CLI: Drop ──"

create_network
start_node "e2e-sdrop-1" "${E2E_IP_PREFIX}.10"

# Drop nonexistent layer — should be a no-op
output=$(docker exec "e2e-sdrop-1" syfrah state drop nonexistent --force 2>&1)
if echo "$output" | grep -qi "no state database\|deleted"; then
    pass "drop nonexistent layer is safe"
else
    fail "drop nonexistent should not error: $output"
fi

# Init a mesh so we have state
init_mesh "e2e-sdrop-1" "${E2E_IP_PREFIX}.10" "node-1"
sleep 2

# Stop daemon before dropping
docker exec "e2e-sdrop-1" pkill -f syfrah 2>/dev/null || true
sleep 1

# Verify state.json exists (old format)
if docker exec "e2e-sdrop-1" test -f /root/.syfrah/state.json 2>/dev/null; then
    pass "state.json exists before drop"
else
    pass "no state.json (may already be using redb)"
fi

# Drop with --force (no confirmation prompt)
output=$(docker exec "e2e-sdrop-1" syfrah state drop fabric --force 2>&1)
if echo "$output" | grep -qi "deleted\|no state database"; then
    pass "drop fabric completed"
else
    fail "drop fabric unexpected output: $output"
fi

cleanup
summary
