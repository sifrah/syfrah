#!/usr/bin/env bash
# Scenario: Catalog integrity verification
#
# Prerequisites:
#   - Docker image built with real images from syfrah-images catalog
#
# Verifies:
#   - catalog.json is valid JSON and has the expected schema
#   - Every image listed in the catalog has a corresponding .raw file on disk
#   - The base_url in the catalog points to a valid GitHub release
#   - The kernel entry exists and the vmlinux file is on disk

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "-- Compute: Catalog Integrity --"

create_network

start_node "e2e-catalog" "172.20.0.10"

# No mesh needed for this test — we only inspect files inside the container.

# -- catalog.json schema ──────────────────────────────────────────────

info "Validating catalog.json schema"
CATALOG=$(docker exec "e2e-catalog" cat /opt/syfrah/catalog.json 2>/dev/null)

if echo "$CATALOG" | jq -e '.' >/dev/null 2>&1; then
    pass "catalog.json is valid JSON"
else
    fail "catalog.json is not valid JSON"
    cleanup
    summary
fi

VERSION=$(echo "$CATALOG" | jq -r '.version // empty')
if [ "$VERSION" = "1" ]; then
    pass "catalog version is 1"
else
    fail "catalog version: '$VERSION' (expected 1)"
fi

BASE_URL=$(echo "$CATALOG" | jq -r '.base_url // empty')
if echo "$BASE_URL" | grep -q "github.com/sacha-ops/syfrah-images"; then
    pass "base_url points to syfrah-images repo"
else
    fail "base_url unexpected: $BASE_URL"
fi

# -- Every catalog image has a local .raw file ────────────────────────

info "Checking all catalog images have local files"
IMAGE_COUNT=$(echo "$CATALOG" | jq '.images | length')
FOUND=0

for name in $(echo "$CATALOG" | jq -r '.images[].name'); do
    if docker exec "e2e-catalog" test -f "/opt/syfrah/images/${name}.raw" 2>/dev/null; then
        SIZE=$(docker exec "e2e-catalog" stat -c%s "/opt/syfrah/images/${name}.raw" 2>/dev/null)
        if [ "$SIZE" -gt 1000 ]; then
            pass "image $name exists on disk ($((SIZE / 1024 / 1024)) MB)"
            FOUND=$((FOUND + 1))
        else
            fail "image $name on disk but too small (${SIZE} bytes)"
        fi
    else
        fail "image $name in catalog but not on disk"
    fi
done

if [ "$FOUND" -eq "$IMAGE_COUNT" ]; then
    pass "all $IMAGE_COUNT catalog images present on disk"
else
    fail "only $FOUND/$IMAGE_COUNT catalog images present"
fi

# -- Kernel entry ─────────────────────────────────────────────────────

info "Checking kernel entry"
KERNEL_FILE=$(echo "$CATALOG" | jq -r '.kernel.file // empty')
if [ -n "$KERNEL_FILE" ]; then
    pass "catalog has kernel entry (file=$KERNEL_FILE)"
else
    fail "catalog missing kernel entry"
fi

KERNEL_VERSION=$(echo "$CATALOG" | jq -r '.kernel.version // empty')
if [ -n "$KERNEL_VERSION" ]; then
    pass "kernel version: $KERNEL_VERSION"
else
    fail "kernel version missing"
fi

if docker exec "e2e-catalog" test -f /opt/syfrah/vmlinux 2>/dev/null; then
    KSIZE=$(docker exec "e2e-catalog" stat -c%s /opt/syfrah/vmlinux 2>/dev/null)
    if [ "$KSIZE" -gt 1000 ]; then
        pass "vmlinux on disk ($((KSIZE / 1024)) KB)"
    else
        fail "vmlinux too small (${KSIZE} bytes)"
    fi
else
    fail "vmlinux not found on disk"
fi

cleanup
summary
