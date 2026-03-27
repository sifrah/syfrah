#!/usr/bin/env bash
# Scenario: CH process crash detection
#
# Prerequisites:
#   - Compute CLI and process monitor must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - When the CH process is killed directly, the monitor detects it
#   - The VM transitions to Failed phase
#   - The failed VM appears correctly in vm list
#   - The failed VM can be deleted and cleaned up

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

# SKIP: Compute CLI is not yet connected to the daemon.
# These scenarios will be enabled once the control socket integration is complete.
echo "SKIP: compute CLI not yet integrated with daemon"
cleanup 2>/dev/null || true
exit 0

echo "── Compute: VM Crash Detection ──"

create_network

start_node "e2e-compute-crash" "172.20.0.10"
init_mesh "e2e-compute-crash" "172.20.0.10" "compute-crash"
sleep 2

# ── Create VM ────────────────────────────────────────────────────

create_vm "e2e-compute-crash" "test-vm-crash" --vcpu 1 --memory 256 --image alpine-3.20
sleep 3

assert_vm_phase "e2e-compute-crash" "test-vm-crash" "Running"

# ── Kill the CH process directly ─────────────────────────────────

CH_PID=$(docker exec "e2e-compute-crash" cat /run/syfrah/vms/test-vm-crash/pid 2>/dev/null)
info "Killing CH process PID $CH_PID"

if [ -n "$CH_PID" ]; then
    docker exec "e2e-compute-crash" kill -9 "$CH_PID" 2>/dev/null || true
    pass "CH process killed"
else
    fail "Could not find CH PID"
fi

# ── Wait for monitor to detect crash ────────────────────────────

info "Waiting for crash detection (up to 15s)"
wait_for_vm_phase "e2e-compute-crash" "test-vm-crash" "Failed" 15

assert_vm_phase "e2e-compute-crash" "test-vm-crash" "Failed"

# ── Verify failed VM in list ────────────────────────────────────

LIST_OUTPUT=$(list_vms "e2e-compute-crash")
if echo "$LIST_OUTPUT" | jq -e '.[] | select(.name == "test-vm-crash")' >/dev/null 2>&1; then
    pass "Failed VM still visible in list"
else
    fail "Failed VM not in list"
fi

# ── Delete the failed VM ────────────────────────────────────────

info "Deleting failed VM"
delete_vm "e2e-compute-crash" "test-vm-crash"
sleep 2

# Verify cleanup
if docker exec "e2e-compute-crash" test -d /run/syfrah/vms/test-vm-crash 2>/dev/null; then
    fail "Runtime directory still exists after deleting failed VM"
else
    pass "Runtime directory cleaned up after deleting failed VM"
fi

assert_vm_count "e2e-compute-crash" 0

cleanup
summary
