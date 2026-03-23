#!/usr/bin/env bash
# Scenario: syfrah state --help and subcommand help work correctly

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── State CLI: Help ──"

create_network
start_node "e2e-shelp-1" "172.20.0.10"

# Main state help
output=$(docker exec "e2e-shelp-1" syfrah state --help 2>&1)
if echo "$output" | grep -q "list" && echo "$output" | grep -q "get" && echo "$output" | grep -q "drop"; then
    pass "state --help shows list, get, drop commands"
else
    fail "state --help missing commands: $output"
fi

# List help
output=$(docker exec "e2e-shelp-1" syfrah state list --help 2>&1)
if echo "$output" | grep -q "layer"; then
    pass "state list --help shows layer argument"
else
    fail "state list --help missing layer: $output"
fi

# Get help
output=$(docker exec "e2e-shelp-1" syfrah state get --help 2>&1)
if echo "$output" | grep -q "table" && echo "$output" | grep -q "key"; then
    pass "state get --help shows table and key arguments"
else
    fail "state get --help missing args: $output"
fi

# Drop help
output=$(docker exec "e2e-shelp-1" syfrah state drop --help 2>&1)
if echo "$output" | grep -q "force"; then
    pass "state drop --help shows --force flag"
else
    fail "state drop --help missing --force: $output"
fi

# Top-level help includes state
output=$(docker exec "e2e-shelp-1" syfrah --help 2>&1)
if echo "$output" | grep -q "state"; then
    pass "syfrah --help includes state command"
else
    fail "syfrah --help missing state: $output"
fi

cleanup
summary
