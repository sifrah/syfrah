#!/usr/bin/env bash
# Scenario: Daemon restart recovery — VM survives daemon restart
#
# Prerequisites:
#   - Compute CLI and daemon reconnect logic must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - After killing the syfrah daemon and restarting, existing VMs are recovered
#   - The CH process survives the daemon restart
#   - syfrah compute vm list shows the VM after recovery
#   - syfrah compute vm get returns correct info

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: VM Reconnect after Daemon Restart ──"

create_network

start_node "e2e-compute-reconn" "172.20.0.10"
init_mesh "e2e-compute-reconn" "172.20.0.10" "compute-reconn"
sleep 2

# ── Create VM ────────────────────────────────────────────────────

create_vm "e2e-compute-reconn" "test-vm-rc" --vcpu 2 --memory 512 --image alpine-3.20
sleep 3

assert_vm_phase "e2e-compute-reconn" "test-vm-rc" "Running"

# Record the CH process PID before restart
CH_PID_BEFORE=$(docker exec "e2e-compute-reconn" cat /run/syfrah/vms/test-vm-rc/pid 2>/dev/null)
info "CH PID before daemon restart: $CH_PID_BEFORE"

# ── Kill daemon (NOT the CH process) ─────────────────────────────

info "Killing syfrah daemon"
DAEMON_PID=$(docker exec "e2e-compute-reconn" cat /root/.syfrah/daemon.pid 2>/dev/null)
if [ -n "$DAEMON_PID" ]; then
    docker exec "e2e-compute-reconn" kill "$DAEMON_PID" 2>/dev/null || true
    sleep 2
    pass "Daemon killed (PID $DAEMON_PID)"
else
    fail "Could not find daemon PID"
fi

# Verify CH process still alive
if [ -n "$CH_PID_BEFORE" ] && docker exec "e2e-compute-reconn" kill -0 "$CH_PID_BEFORE" 2>/dev/null; then
    pass "CH process $CH_PID_BEFORE survived daemon kill"
else
    fail "CH process died with daemon"
fi

# ── Restart daemon ───────────────────────────────────────────────

info "Restarting syfrah daemon"
docker exec -d "e2e-compute-reconn" \
    syfrah fabric init \
    --name "$E2E_MESH" \
    --node-name "compute-reconn" \
    --endpoint "172.20.0.10:51820"

wait_daemon "e2e-compute-reconn"
sleep 3

# ── Verify VM recovered ─────────────────────────────────────────

LIST_OUTPUT=$(list_vms "e2e-compute-reconn")
if echo "$LIST_OUTPUT" | jq -e '.[] | select(.name == "test-vm-rc")' >/dev/null 2>&1; then
    pass "VM test-vm-rc recovered after daemon restart"
else
    fail "VM test-vm-rc not found after daemon restart: $LIST_OUTPUT"
fi

assert_vm_phase "e2e-compute-reconn" "test-vm-rc" "Running"

# ── Verify same CH PID ───────────────────────────────────────────

CH_PID_AFTER=$(docker exec "e2e-compute-reconn" cat /run/syfrah/vms/test-vm-rc/pid 2>/dev/null)
if [ "$CH_PID_BEFORE" = "$CH_PID_AFTER" ]; then
    pass "CH PID unchanged after daemon restart ($CH_PID_BEFORE)"
else
    fail "CH PID changed: $CH_PID_BEFORE -> $CH_PID_AFTER"
fi

cleanup
summary
