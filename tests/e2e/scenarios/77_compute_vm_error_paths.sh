#!/usr/bin/env bash
# Scenario: Compute error handling
#
# Prerequisites:
#   - Compute CLI (syfrah compute vm create/stop/delete) must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - Creating a VM with invalid spec (vcpus=0) returns an error
#   - Stopping a non-existent VM returns an error or not-found
#   - Deleting a non-existent VM returns an error or succeeds (idempotent)
#   - Creating a duplicate-named VM returns an error
#   - No leaked processes or runtime dirs after errors

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: VM Error Paths ──"

create_network

start_node "e2e-compute-errors" "172.20.0.10"
init_mesh "e2e-compute-errors" "172.20.0.10" "compute-errors"
sleep 2

# ── Invalid spec: vcpus=0 ───────────────────────────────────────

info "Creating VM with vcpus=0 (should fail)"
OUTPUT=$(create_vm "e2e-compute-errors" "bad-vm" --vcpu 0 --memory 256 --image alpine-3.20 2>&1)
EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ]; then
    pass "VM creation with vcpus=0 failed as expected"
else
    fail "VM creation with vcpus=0 should have failed: $OUTPUT"
fi

# ── Stop non-existent VM ────────────────────────────────────────

info "Stopping non-existent VM (should fail)"
OUTPUT=$(stop_vm "e2e-compute-errors" "ghost-vm" 2>&1)
EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ]; then
    pass "Stopping non-existent VM failed as expected"
else
    # Some implementations may return success with a message; check output
    if echo "$OUTPUT" | grep -qi "not found\|no such\|does not exist"; then
        pass "Stopping non-existent VM returned not-found message"
    else
        fail "Stopping non-existent VM unexpectedly succeeded: $OUTPUT"
    fi
fi

# ── Delete non-existent VM ──────────────────────────────────────

info "Deleting non-existent VM"
OUTPUT=$(delete_vm "e2e-compute-errors" "ghost-vm" 2>&1)
EXIT_CODE=$?

# Delete of non-existent may be idempotent (success) or return not-found
if [ $EXIT_CODE -eq 0 ]; then
    pass "Deleting non-existent VM returned success (idempotent)"
elif echo "$OUTPUT" | grep -qi "not found\|no such\|does not exist"; then
    pass "Deleting non-existent VM returned not-found"
else
    fail "Deleting non-existent VM unexpected result ($EXIT_CODE): $OUTPUT"
fi

# ── Duplicate VM name ────────────────────────────────────────────

info "Creating VM, then creating duplicate"
create_vm "e2e-compute-errors" "dup-vm" --vcpu 1 --memory 256 --image alpine-3.20
sleep 3

OUTPUT=$(create_vm "e2e-compute-errors" "dup-vm" --vcpu 1 --memory 256 --image alpine-3.20 2>&1)
EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ]; then
    pass "Duplicate VM creation failed as expected"
else
    fail "Duplicate VM creation should have failed: $OUTPUT"
fi

# ── Verify no leaked processes or dirs from failed attempts ──────

if docker exec "e2e-compute-errors" test -d /run/syfrah/vms/bad-vm 2>/dev/null; then
    fail "Runtime directory leaked for bad-vm"
else
    pass "No leaked runtime directory for bad-vm"
fi

if docker exec "e2e-compute-errors" test -d /run/syfrah/vms/ghost-vm 2>/dev/null; then
    fail "Runtime directory leaked for ghost-vm"
else
    pass "No leaked runtime directory for ghost-vm"
fi

cleanup
summary
