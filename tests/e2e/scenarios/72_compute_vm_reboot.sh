#!/usr/bin/env bash
# Scenario: VM reboot
#
# Prerequisites:
#   - Compute CLI (syfrah compute vm create/list) and reboot support
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - A running VM can be rebooted
#   - The VM returns to Running phase after reboot

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

# SKIP: Compute CLI is not yet connected to the daemon.
# These scenarios will be enabled once the control socket integration is complete.
echo "SKIP: compute CLI not yet integrated with daemon"
cleanup 2>/dev/null || true
exit 0

echo "── Compute: VM Reboot ──"

create_network

start_node "e2e-compute-reboot" "172.20.0.10"
init_mesh "e2e-compute-reboot" "172.20.0.10" "compute-reboot"
sleep 2

# ── Create VM ────────────────────────────────────────────────────

create_vm "e2e-compute-reboot" "test-vm-rb" --vcpu 1 --memory 256 --image alpine-3.20
sleep 3

assert_vm_phase "e2e-compute-reboot" "test-vm-rb" "Running"

# ── Reboot the VM ────────────────────────────────────────────────

info "Rebooting VM"
OUTPUT=$(docker exec "e2e-compute-reboot" syfrah compute vm reboot "test-vm-rb" 2>&1)
if [ $? -eq 0 ]; then
    pass "VM reboot command succeeded"
else
    fail "VM reboot command failed: $OUTPUT"
fi

# ── Verify VM returns to Running ─────────────────────────────────

sleep 3

assert_vm_phase "e2e-compute-reboot" "test-vm-rb" "Running"

# ── Verify PID is still alive (fake CH survives reboot) ──────────

PID=$(docker exec "e2e-compute-reboot" cat /run/syfrah/vms/test-vm-rb/pid 2>/dev/null)
if [ -n "$PID" ] && docker exec "e2e-compute-reboot" kill -0 "$PID" 2>/dev/null; then
    pass "CH process still alive after reboot"
else
    fail "CH process not alive after reboot"
fi

cleanup
summary
