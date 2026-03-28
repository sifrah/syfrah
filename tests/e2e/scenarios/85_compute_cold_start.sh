#!/usr/bin/env bash
# Scenario: Cold start — fresh server, no pre-installed images
# Tests the REAL operator onboarding flow: catalog → pull → create → delete
# NOTHING is pre-installed. Everything goes through the real CLI.

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Compute: Cold Start (Real Operator Flow) ──"

# ── Helper: retry a GitHub-facing command up to 3 times (5 s apart) ──
catalog_with_retry() {
    local node="$1"
    local attempt=0
    local output=""
    while [ $attempt -lt 3 ]; do
        output=$(docker exec "$node" syfrah compute image catalog --json 2>&1)
        local count
        count=$(echo "$output" | jq '.images | length' 2>/dev/null || echo "0")
        if [ "$count" -gt 0 ]; then
            echo "$output"
            return 0
        fi
        attempt=$((attempt + 1))
        [ $attempt -lt 3 ] && sleep 5
    done
    echo "$output"
    return 1
}

pull_with_retry() {
    local node="$1" name="$2"
    local attempt=0
    local output=""
    while [ $attempt -lt 3 ]; do
        if docker exec "$node" syfrah compute image pull "$name" 2>&1; then
            # Verify the image actually appears in the local store
            if docker exec "$node" syfrah compute image list --json 2>&1 \
                | jq -e ".[] | select(.name == \"$name\")" >/dev/null 2>&1; then
                return 0
            fi
        fi
        attempt=$((attempt + 1))
        [ $attempt -lt 3 ] && sleep 5
    done
    return 1
}

create_network
start_node "e2e-cold-1" "172.20.0.10"
init_mesh "e2e-cold-1" "172.20.0.10" "cold-node"

# Stop daemon FIRST so it can't write anything back while we clean
docker exec "e2e-cold-1" syfrah fabric stop 2>/dev/null || true
sleep 1
# Clear pre-installed images to simulate cold start
docker exec "e2e-cold-1" sh -c 'rm -f /opt/syfrah/images/*.raw /opt/syfrah/images/*.raw.tmp /opt/syfrah/images/images.json /opt/syfrah/images/catalog.json /opt/syfrah/images/.lock'
# Verify the directory is actually empty (minus dotfiles)
docker exec "e2e-cold-1" sh -c 'ls /opt/syfrah/images/ 2>/dev/null || true'
# Restart daemon so it reloads the now-empty store
docker exec -d "e2e-cold-1" syfrah fabric start
wait_daemon "e2e-cold-1" 30

node="e2e-cold-1"

# Step 1: Image list should be empty
info "Step 1: Verify no images on fresh install"
LIST=$(docker exec "$node" syfrah compute image list --json 2>&1)
if echo "$LIST" | jq -e '. | length == 0' >/dev/null 2>&1; then
    pass "No images on fresh install"
else
    fail "Expected empty image list, got: $LIST"
fi

# Step 2: Catalog should show images from the real GitHub catalog (with retries)
info "Step 2: Fetch remote catalog (up to 3 attempts)"
CATALOG=$(catalog_with_retry "$node")
CATALOG_COUNT=$(echo "$CATALOG" | jq '.images | length' 2>/dev/null || echo "0")
if [ "$CATALOG_COUNT" -gt 0 ]; then
    pass "Catalog shows $CATALOG_COUNT images"
else
    fail "Catalog is empty or unreachable after 3 attempts: $CATALOG"
fi

# Step 3: Pull a real image (with retries + verify in image list)
info "Step 3: Pull alpine-3.20 from catalog (up to 3 attempts)"
if pull_with_retry "$node" "alpine-3.20"; then
    pass "Image pull succeeded and verified in image list"
else
    fail "Image pull failed or image not found in list after 3 attempts"
fi

# Step 4: Verify image appears in list (explicit JSON check)
info "Step 4: Verify image in local list"
LIST_AFTER=$(docker exec "$node" syfrah compute image list --json 2>&1)
if echo "$LIST_AFTER" | jq -e '.[] | select(.name == "alpine-3.20")' >/dev/null 2>&1; then
    pass "alpine-3.20 appears in local list"
else
    fail "alpine-3.20 not found after pull: $LIST_AFTER"
fi

# Step 5: Create a VM with the pulled image (KVM-aware)
info "Step 5: Create VM with pulled image"
CREATE_OUTPUT=$(docker exec "$node" syfrah compute vm create --name cold-test --image alpine-3.20 --vcpus 1 --memory 256 2>&1) || CREATE_RC=$?
CREATE_RC=${CREATE_RC:-0}

if docker exec "$node" test -e /dev/kvm 2>/dev/null; then
    # KVM available — VM should reach Running
    if [ $CREATE_RC -eq 0 ]; then
        wait_for_vm_phase "$node" "cold-test" "Running" 30
        pass "VM reached Running on KVM-capable host"
    else
        fail "VM creation failed on KVM-capable host: $CREATE_OUTPUT"
    fi
else
    # No KVM — expect a clear error mentioning KVM / not available / degraded
    if echo "$CREATE_OUTPUT" | grep -qi "kvm\|not available\|degraded"; then
        pass "VM creation correctly reported no KVM"
    else
        fail "VM creation failed with unclear error: $CREATE_OUTPUT"
    fi
fi

# Step 6: Cleanup
info "Step 6: Cleanup"
docker exec "$node" syfrah compute vm delete cold-test --yes 2>/dev/null || true
docker exec "$node" syfrah compute image delete alpine-3.20 --yes 2>/dev/null || true

cleanup
summary
