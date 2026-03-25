#!/usr/bin/env bash
# Scenario: Corrupted state files — daemon must fail cleanly, not panic
# Now tests both state.json AND fabric.redb corruption

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── State File Corruption ──"

create_network
start_node "e2e-corrupt-1" "172.20.0.10"

# Init a mesh so we have valid state
init_mesh "e2e-corrupt-1" "172.20.0.10" "node-1"
sleep 2

# Stop daemon
docker exec "e2e-corrupt-1" pkill -f syfrah 2>/dev/null || true
sleep 2

# Sub-test A: truncated JSON + remove redb
info "Test A: truncated JSON (no redb fallback)..."
docker exec "e2e-corrupt-1" sh -c 'rm -f /root/.syfrah/fabric.redb; echo "{\"mesh_na" > /root/.syfrah/state.json'
assert_command_fails "e2e-corrupt-1" syfrah fabric start

# Sub-test B: empty JSON + remove redb
info "Test B: empty file (no redb fallback)..."
docker exec "e2e-corrupt-1" sh -c 'rm -f /root/.syfrah/fabric.redb; > /root/.syfrah/state.json'
assert_command_fails "e2e-corrupt-1" syfrah fabric start

# Sub-test C: binary garbage + remove redb
info "Test C: binary garbage (no redb fallback)..."
docker exec "e2e-corrupt-1" sh -c 'rm -f /root/.syfrah/fabric.redb; dd if=/dev/urandom of=/root/.syfrah/state.json bs=256 count=1 2>/dev/null'
assert_command_fails "e2e-corrupt-1" syfrah fabric start

# Recovery: leave cleans up, then re-init works
info "Recovery: leave + re-init..."
docker exec "e2e-corrupt-1" syfrah fabric leave --yes 2>/dev/null || true
docker exec "e2e-corrupt-1" rm -rf /root/.syfrah 2>/dev/null || true
init_mesh "e2e-corrupt-1" "172.20.0.10" "node-1"
assert_daemon_running "e2e-corrupt-1"

cleanup
summary
