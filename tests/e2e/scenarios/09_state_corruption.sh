#!/usr/bin/env bash
# Scenario: Corrupted state.json — daemon must fail cleanly, not panic

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── State File Corruption ──"

create_network
start_node "e2e-corrupt-1" "172.20.0.10"

# Init a mesh so we have a valid state
init_mesh "e2e-corrupt-1" "172.20.0.10" "node-1"
sleep 2

# Stop daemon
docker exec "e2e-corrupt-1" pkill -f syfrah 2>/dev/null || true
sleep 2

# Sub-test A: truncated JSON
info "Test A: truncated JSON..."
docker exec "e2e-corrupt-1" sh -c 'echo "{\"mesh_na" > /root/.syfrah/state.json'
assert_command_fails "e2e-corrupt-1" syfrah fabric start

# Sub-test B: empty file
info "Test B: empty file..."
docker exec "e2e-corrupt-1" sh -c '> /root/.syfrah/state.json'
assert_command_fails "e2e-corrupt-1" syfrah fabric start

# Sub-test C: binary garbage
info "Test C: binary garbage..."
docker exec "e2e-corrupt-1" sh -c 'dd if=/dev/urandom of=/root/.syfrah/state.json bs=256 count=1 2>/dev/null'
assert_command_fails "e2e-corrupt-1" syfrah fabric start

# Recovery: leave cleans up, then re-init works
info "Recovery: leave + re-init..."
docker exec "e2e-corrupt-1" syfrah fabric leave 2>/dev/null || true
docker exec "e2e-corrupt-1" rm -rf /root/.syfrah 2>/dev/null || true
init_mesh "e2e-corrupt-1" "172.20.0.10" "node-1"
assert_daemon_running "e2e-corrupt-1"

cleanup
summary
