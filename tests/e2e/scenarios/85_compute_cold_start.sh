#!/usr/bin/env bash
# Scenario: Cold start — fresh server, no pre-installed images
# Tests the REAL operator onboarding flow: catalog → pull → create → delete
# NOTHING is pre-installed. Everything goes through the real CLI.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: Cold Start (Real Operator Flow) ──"

create_network
start_node "e2e-cold-1" "172.20.0.10"
init_mesh "e2e-cold-1" "172.20.0.10" "cold-node"

sleep 2

# Step 1: Image list should be empty
info "Step 1: Verify no images on fresh install"
LIST=$(docker exec "e2e-cold-1" syfrah compute image list --json 2>&1)
if echo "$LIST" | jq -e '. | length == 0' >/dev/null 2>&1; then
    pass "No images on fresh install"
else
    fail "Expected empty image list, got: $LIST"
fi

# Step 2: Catalog should show images from the real GitHub catalog
info "Step 2: Fetch remote catalog"
CATALOG=$(docker exec "e2e-cold-1" syfrah compute image catalog --json 2>&1)
CATALOG_COUNT=$(echo "$CATALOG" | jq '.images | length' 2>/dev/null || echo "0")
if [ "$CATALOG_COUNT" -gt 0 ]; then
    pass "Catalog shows $CATALOG_COUNT images"
else
    fail "Catalog is empty or unreachable: $CATALOG"
fi

# Step 3: Pull a real image
info "Step 3: Pull alpine-3.20 from catalog"
PULL_OUTPUT=$(docker exec "e2e-cold-1" syfrah compute image pull alpine-3.20 2>&1)
if echo "$PULL_OUTPUT" | grep -qi "done\|success\|pulled"; then
    pass "Image pull succeeded"
else
    fail "Image pull failed: $PULL_OUTPUT"
fi

# Step 4: Verify image appears in list
info "Step 4: Verify image in local list"
LIST_AFTER=$(docker exec "e2e-cold-1" syfrah compute image list --json 2>&1)
if echo "$LIST_AFTER" | jq -e '.[] | select(.name == "alpine-3.20")' >/dev/null 2>&1; then
    pass "alpine-3.20 appears in local list"
else
    fail "alpine-3.20 not found after pull: $LIST_AFTER"
fi

# Step 5: Create a VM with the pulled image
info "Step 5: Create VM with pulled image"
CREATE_OUTPUT=$(docker exec "e2e-cold-1" syfrah compute vm create --name cold-test --image alpine-3.20 --vcpus 1 --memory 256 2>&1)
if echo "$CREATE_OUTPUT" | grep -qi "created\|running"; then
    pass "VM created with pulled image"
else
    # VM creation may fail due to no KVM in Docker — that's expected
    if echo "$CREATE_OUTPUT" | grep -qi "kvm\|not available"; then
        pass "VM creation failed as expected (no KVM in Docker): correct error"
    else
        fail "VM creation failed unexpectedly: $CREATE_OUTPUT"
    fi
fi

# Step 6: Cleanup
info "Step 6: Cleanup"
docker exec "e2e-cold-1" syfrah compute vm delete cold-test --yes 2>/dev/null || true
docker exec "e2e-cold-1" syfrah compute image delete alpine-3.20 --yes 2>/dev/null || true

cleanup
summary
