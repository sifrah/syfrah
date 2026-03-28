#!/usr/bin/env bash
# Scenario: Force stop a VM
#
# Prerequisites:
#   - Compute CLI (syfrah compute vm create/stop --force) must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - A running VM can be force-stopped
#   - Force stop completes quickly (< 5 seconds)
#   - The VM reaches Stopped phase

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: VM Force Stop ──"

create_network

start_node "e2e-compute-fstop" "172.20.0.10"
init_mesh "e2e-compute-fstop" "172.20.0.10" "compute-fstop"
sleep 2

# ── Create VM ────────────────────────────────────────────────────

create_vm "e2e-compute-fstop" "test-vm-fs" --vcpu 1 --memory 256 --image alpine-3.20
sleep 3

assert_vm_phase "e2e-compute-fstop" "test-vm-fs" "Running"

# ── Force stop ───────────────────────────────────────────────────

info "Force stopping VM"
START_TIME=$(date +%s)

OUTPUT=$(stop_vm "e2e-compute-fstop" "test-vm-fs" --force)
EXIT_CODE=$?

ELAPSED=$(( $(date +%s) - START_TIME ))

if [ $EXIT_CODE -eq 0 ]; then
    pass "Force stop command succeeded"
else
    fail "Force stop command failed: $OUTPUT"
fi

if [ "$ELAPSED" -lt 5 ]; then
    pass "Force stop completed in ${ELAPSED}s (< 5s)"
else
    fail "Force stop took ${ELAPSED}s (expected < 5s)"
fi

# ── Verify Stopped phase ────────────────────────────────────────

sleep 2
assert_vm_phase "e2e-compute-fstop" "test-vm-fs" "Stopped"

# ── Verify CH process is gone ────────────────────────────────────

PID=$(docker exec "e2e-compute-fstop" cat /run/syfrah/vms/test-vm-fs/pid 2>/dev/null) || true
if [ -n "$PID" ] && docker exec "e2e-compute-fstop" kill -0 "$PID" 2>/dev/null; then
    fail "CH process $PID still alive after force stop"
else
    pass "CH process terminated after force stop"
fi

cleanup
summary
