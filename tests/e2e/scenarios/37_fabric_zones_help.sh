#!/usr/bin/env bash
# Scenario: --region and --zone flags appear in CLI help

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Zones: CLI Help ──"

create_network
start_node "e2e-zhelp-1" "${E2E_IP_PREFIX}.10"

# Init help shows --region and --zone
output=$(docker exec "e2e-zhelp-1" syfrah fabric init --help 2>&1)
if echo "$output" | grep -q "\-\-region"; then
    pass "init --help shows --region flag"
else
    fail "init --help missing --region"
fi

if echo "$output" | grep -q "\-\-zone"; then
    pass "init --help shows --zone flag"
else
    fail "init --help missing --zone"
fi

# Join help shows --region and --zone
output=$(docker exec "e2e-zhelp-1" syfrah fabric join --help 2>&1)
if echo "$output" | grep -q "\-\-region"; then
    pass "join --help shows --region flag"
else
    fail "join --help missing --region"
fi

if echo "$output" | grep -q "\-\-zone"; then
    pass "join --help shows --zone flag"
else
    fail "join --help missing --zone"
fi

cleanup
summary
