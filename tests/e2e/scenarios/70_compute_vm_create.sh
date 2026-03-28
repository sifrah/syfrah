#!/usr/bin/env bash
# Scenario: VM creation lifecycle
#
# Prerequisites:
#   - Compute CLI (syfrah compute vm create/list/get) must be implemented
#   - ComputeHandler must be integrated into the daemon
#   - Fake cloud-hypervisor must be installed in the Docker image
#
# Verifies:
#   - A VM can be created with syfrah compute vm create
#   - The VM appears in syfrah compute vm list
#   - The VM phase is Running
#   - syfrah compute vm get returns correct fields (vcpus, memory)
#   - Runtime directory exists with valid metadata
#   - PID file exists and the process is alive

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: VM Create ──"

create_network

start_node "e2e-compute-create" "172.20.0.10"
init_mesh "e2e-compute-create" "172.20.0.10" "compute-create"
sleep 2

# ── Create a VM ──────────────────────────────────────────────────

info "Creating test VM"
OUTPUT=$(create_vm "e2e-compute-create" "test-vm-1" --vcpu 2 --memory 512 --image alpine-3.20 || true)

if echo "$OUTPUT" | grep -qi "VM created\|Running"; then
    pass "VM creation command succeeded"
else
    fail "VM creation command failed: $OUTPUT"
    info "Daemon log:"
    docker exec "e2e-compute-create" cat /root/.syfrah/syfrah.log 2>/dev/null | tail -30 || true
    info "Processes:"
    docker exec "e2e-compute-create" ps aux 2>/dev/null || true
fi

# ── Verify VM in list ────────────────────────────────────────────

sleep 3  # allow VM to reach Running state

LIST_OUTPUT=$(list_vms "e2e-compute-create")
if echo "$LIST_OUTPUT" | jq -e '.[] | select(.name == "test-vm-1")' >/dev/null 2>&1; then
    pass "VM test-vm-1 appears in vm list"
else
    fail "VM test-vm-1 not in vm list: $LIST_OUTPUT"
fi

# ── Verify VM phase ─────────────────────────────────────────────

assert_vm_phase "e2e-compute-create" "test-vm-1" "Running"

# ── Verify VM details ───────────────────────────────────────────

VM_JSON=$(get_vm "e2e-compute-create" "test-vm-1")

VCPUS=$(echo "$VM_JSON" | jq -r '.vcpus // .spec.vcpus // empty' 2>/dev/null)
if [ "$VCPUS" = "2" ]; then
    pass "VM has 2 vCPUs"
else
    fail "VM vCPUs: $VCPUS (expected 2)"
fi

MEMORY=$(echo "$VM_JSON" | jq -r '.memory // .spec.memory // empty' 2>/dev/null)
if [ "$MEMORY" = "512" ]; then
    pass "VM has 512 MB memory"
else
    fail "VM memory: $MEMORY (expected 512)"
fi

# ── Verify runtime directory ────────────────────────────────────

if docker exec "e2e-compute-create" test -d /run/syfrah/vms/test-vm-1 2>/dev/null; then
    pass "Runtime directory exists"
else
    fail "Runtime directory /run/syfrah/vms/test-vm-1 missing"
fi

# ── Verify meta.json ────────────────────────────────────────────

META=$(docker exec "e2e-compute-create" cat /run/syfrah/vms/test-vm-1/meta.json 2>/dev/null)
if echo "$META" | jq -e . >/dev/null 2>&1; then
    pass "meta.json is valid JSON"
else
    fail "meta.json invalid or missing: $META"
fi

# ── Verify PID file and process ──────────────────────────────────

PID=$(docker exec "e2e-compute-create" cat /run/syfrah/vms/test-vm-1/pid 2>/dev/null)
if [ -n "$PID" ]; then
    if docker exec "e2e-compute-create" kill -0 "$PID" 2>/dev/null; then
        pass "CH process $PID is alive"
    else
        fail "CH process $PID is not alive"
    fi
else
    fail "PID file missing or empty"
fi

cleanup
summary
