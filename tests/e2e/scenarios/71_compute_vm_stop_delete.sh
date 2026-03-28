#!/usr/bin/env bash
# Scenario: VM stop and delete lifecycle
#
# Prerequisites:
#   - Compute CLI (syfrah compute vm create/stop/delete/list) must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - A running VM can be stopped
#   - A stopped VM reaches Stopped phase
#   - A VM can be deleted
#   - Deleted VM no longer appears in list
#   - Runtime directory is cleaned up after delete

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: VM Stop & Delete ──"

create_network

start_node "e2e-compute-stopdel" "172.20.0.10"
init_mesh "e2e-compute-stopdel" "172.20.0.10" "compute-stopdel"
sleep 2

# ── Create and verify VM is running ─────────────────────────────

create_vm "e2e-compute-stopdel" "test-vm-sd" --vcpu 1 --memory 256 --image alpine-3.20
sleep 3

assert_vm_phase "e2e-compute-stopdel" "test-vm-sd" "Running"

# ── Stop the VM ──────────────────────────────────────────────────

info "Stopping VM"
OUTPUT=$(stop_vm "e2e-compute-stopdel" "test-vm-sd")
if [ $? -eq 0 ]; then
    pass "VM stop command succeeded"
else
    fail "VM stop command failed: $OUTPUT"
fi

wait_for_vm_phase "e2e-compute-stopdel" "test-vm-sd" "Stopped" 15

assert_vm_phase "e2e-compute-stopdel" "test-vm-sd" "Stopped"

# ── Delete the VM ────────────────────────────────────────────────

info "Deleting VM"
OUTPUT=$(delete_vm "e2e-compute-stopdel" "test-vm-sd")
if [ $? -eq 0 ]; then
    pass "VM delete command succeeded"
else
    fail "VM delete command failed: $OUTPUT"
fi

sleep 2

# ── Verify VM gone from list ────────────────────────────────────

LIST_OUTPUT=$(list_vms "e2e-compute-stopdel")
if echo "$LIST_OUTPUT" | jq -e '.[] | select(.id == "test-vm-sd")' >/dev/null 2>&1; then
    fail "VM test-vm-sd still in list after delete"
else
    pass "VM test-vm-sd removed from list"
fi

# ── Verify runtime directory cleaned up ──────────────────────────

if docker exec "e2e-compute-stopdel" test -d /run/syfrah/vms/test-vm-sd 2>/dev/null; then
    fail "Runtime directory still exists after delete"
else
    pass "Runtime directory cleaned up"
fi

cleanup
summary
