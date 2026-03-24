#!/usr/bin/env bash
# Scenario: UX Errors — no raw Rust errors in any output
# Runs every command in wrong states and verifies no raw errors leak.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── UX Errors: No Raw Errors ──"
trap cleanup EXIT
create_network

start_node "e2e-err-raw-1" "172.20.0.10"

FORBIDDEN_PATTERNS="os error|anyhow|panicked|thread 'main'|early eof|RUST_BACKTRACE|unwrap()"

# Test all commands in clean state (no mesh)
info "Testing: commands with no mesh..."
for cmd in \
    "syfrah fabric peers" \
    "syfrah fabric status" \
    "syfrah fabric stop" \
    "syfrah fabric start" \
    "syfrah fabric leave" \
    "syfrah fabric token" \
    "syfrah --help" \
    "syfrah fabric --help"; do
    output=$(docker exec "e2e-err-raw-1" sh -c "$cmd" 2>&1 || true)
    if echo "$output" | grep -qEi "$FORBIDDEN_PATTERNS"; then
        fail "$cmd (no mesh): contains raw error"
    else
        pass "$cmd (no mesh): clean output"
    fi
done

# Init mesh and test commands in running state
info "Testing: commands with running mesh..."
init_mesh "e2e-err-raw-1" "172.20.0.10" "raw-node-1"

for cmd in \
    "syfrah fabric peers" \
    "syfrah fabric status" \
    "syfrah fabric token"; do
    output=$(docker exec "e2e-err-raw-1" sh -c "$cmd" 2>&1 || true)
    if echo "$output" | grep -qEi "$FORBIDDEN_PATTERNS"; then
        fail "$cmd (running): contains raw error"
    else
        pass "$cmd (running): clean output"
    fi
done

# Test join to bad target
info "Testing: join to bad target..."
output=$(docker exec "e2e-err-raw-1" sh -c \
    "syfrah fabric join 10.99.99.99:51821 --node-name test --endpoint 172.20.0.10:51820" 2>&1 || true)
if echo "$output" | grep -qEi "$FORBIDDEN_PATTERNS"; then
    fail "join bad target: contains raw error"
else
    pass "join bad target: clean output"
fi

# Test double init
info "Testing: double init..."
output=$(docker exec "e2e-err-raw-1" sh -c \
    "syfrah fabric init --name test2 --node-name n2 --endpoint 172.20.0.10:51820" 2>&1 || true)
if echo "$output" | grep -qEi "$FORBIDDEN_PATTERNS"; then
    fail "double init: contains raw error"
else
    pass "double init: clean output"
fi

cleanup
summary
