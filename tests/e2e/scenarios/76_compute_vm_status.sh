#!/usr/bin/env bash
# Scenario: Compute status endpoint with mixed VM states
#
# Prerequisites:
#   - Compute CLI (syfrah compute status, vm create/stop) must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - syfrah compute status reports correct total VM count
#   - syfrah compute status reports correct running VM count
#   - Counts update correctly when VMs are stopped

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: VM Status Counts ──"

create_network

start_node "e2e-compute-status" "172.20.0.10"
init_mesh "e2e-compute-status" "172.20.0.10" "compute-status"
sleep 2

# ── Create 2 VMs ────────────────────────────────────────────────

create_vm "e2e-compute-status" "vm-run" --vcpu 1 --memory 256 --image alpine-3.20
create_vm "e2e-compute-status" "vm-stop" --vcpu 1 --memory 256 --image alpine-3.20
sleep 5

assert_vm_phase "e2e-compute-status" "vm-run"  "Running"
assert_vm_phase "e2e-compute-status" "vm-stop" "Running"

# ── Stop one VM ──────────────────────────────────────────────────

stop_vm "e2e-compute-status" "vm-stop"
wait_for_vm_phase "e2e-compute-status" "vm-stop" "Stopped" 15

# ── Check compute status ─────────────────────────────────────────

info "Checking compute status"
STATUS_JSON=$(docker exec "e2e-compute-status" syfrah compute status --json 2>&1)

TOTAL=$(echo "$STATUS_JSON" | jq -r '.total_vms // empty' 2>/dev/null)
RUNNING=$(echo "$STATUS_JSON" | jq -r '.running_vms // empty' 2>/dev/null)

if [ "$TOTAL" = "2" ]; then
    pass "total_vms = 2"
else
    fail "total_vms = $TOTAL (expected 2)"
fi

if [ "$RUNNING" = "1" ]; then
    pass "running_vms = 1"
else
    fail "running_vms = $RUNNING (expected 1)"
fi

cleanup
summary
