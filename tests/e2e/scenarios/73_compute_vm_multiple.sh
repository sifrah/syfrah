#!/usr/bin/env bash
# Scenario: Multiple VMs on a single node
#
# Prerequisites:
#   - Compute CLI (syfrah compute vm create/stop/delete/list) must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - 3 VMs with different specs can be created
#   - All 3 appear in vm list
#   - Stopping one does not affect others
#   - Deleting one does not affect others
#   - Counts are correct at each step

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: Multiple VMs ──"

create_network

start_node "e2e-compute-multi" "172.20.0.10"
init_mesh "e2e-compute-multi" "172.20.0.10" "compute-multi"
sleep 2

# ── Create 3 VMs ────────────────────────────────────────────────

info "Creating 3 VMs"
create_vm "e2e-compute-multi" "vm-alpha" --vcpu 1 --memory 256 --image alpine-3.20
create_vm "e2e-compute-multi" "vm-beta"  --vcpu 2 --memory 512 --image alpine-3.20
create_vm "e2e-compute-multi" "vm-gamma" --vcpu 4 --memory 1024 --image alpine-3.20
sleep 5

# ── Verify all 3 in list ────────────────────────────────────────

assert_vm_count "e2e-compute-multi" 3

assert_vm_phase "e2e-compute-multi" "vm-alpha" "Running"
assert_vm_phase "e2e-compute-multi" "vm-beta"  "Running"
assert_vm_phase "e2e-compute-multi" "vm-gamma" "Running"

# ── Stop one VM ──────────────────────────────────────────────────

info "Stopping vm-beta"
stop_vm "e2e-compute-multi" "vm-beta"
wait_for_vm_phase "e2e-compute-multi" "vm-beta" "Stopped" 15

assert_vm_phase "e2e-compute-multi" "vm-alpha" "Running"
assert_vm_phase "e2e-compute-multi" "vm-beta"  "Stopped"
assert_vm_phase "e2e-compute-multi" "vm-gamma" "Running"

# ── Delete the stopped VM ────────────────────────────────────────

info "Deleting vm-beta"
delete_vm "e2e-compute-multi" "vm-beta"
sleep 2

assert_vm_count "e2e-compute-multi" 2

# ── Remaining VMs unaffected ─────────────────────────────────────

assert_vm_phase "e2e-compute-multi" "vm-alpha" "Running"
assert_vm_phase "e2e-compute-multi" "vm-gamma" "Running"

cleanup
summary
