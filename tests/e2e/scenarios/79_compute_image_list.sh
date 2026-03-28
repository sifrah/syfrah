#!/usr/bin/env bash
# Scenario: Image listing with real images from catalog
#
# Prerequisites:
#   - Docker image built with real images from syfrah-images catalog
#   - Compute CLI (syfrah compute image list) must be implemented
#
# Verifies:
#   - syfrah compute image list returns images
#   - alpine-3.20 appears in the list (pre-downloaded from catalog)
#   - JSON output is valid and contains expected fields
#   - The image file on disk is a real raw image (non-zero size)

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "-- Compute: Image List (real images) --"

create_network

start_node "e2e-image-list" "172.20.0.10"
init_mesh "e2e-image-list" "172.20.0.10" "image-list"
sleep 2

# -- Verify real image file exists on disk (from Docker build) -------

info "Checking that real alpine-3.20.raw exists"
SIZE=$(docker exec "e2e-image-list" stat -c%s /opt/syfrah/images/alpine-3.20.raw 2>/dev/null || echo "0")
if [ "$SIZE" -gt 1000000 ]; then
    pass "alpine-3.20.raw is a real image ($((SIZE / 1024 / 1024)) MB)"
else
    fail "alpine-3.20.raw is missing or too small (${SIZE} bytes)"
fi

# -- Verify kernel file exists ----------------------------------------

KERNEL_SIZE=$(docker exec "e2e-image-list" stat -c%s /opt/syfrah/vmlinux 2>/dev/null || echo "0")
if [ "$KERNEL_SIZE" -gt 1000 ]; then
    pass "vmlinux kernel is a real file ($((KERNEL_SIZE / 1024)) KB)"
else
    fail "vmlinux is missing or too small (${KERNEL_SIZE} bytes)"
fi

# -- List images via CLI ─────────────────────────────────────────────

info "Listing images via CLI"
LIST_OUTPUT=$(list_images "e2e-image-list")

if echo "$LIST_OUTPUT" | grep -q "alpine-3.20"; then
    pass "alpine-3.20 appears in image list"
else
    fail "alpine-3.20 not in image list: $LIST_OUTPUT"
fi

# -- JSON output ──────────────────────────────────────────────────────

info "Listing images as JSON"
JSON_OUTPUT=$(list_images "e2e-image-list" --json)

if echo "$JSON_OUTPUT" | jq -e '.' >/dev/null 2>&1; then
    pass "JSON output is valid"
else
    fail "JSON output is not valid JSON: $JSON_OUTPUT"
fi

NAME=$(echo "$JSON_OUTPUT" | jq -r '.[] | select(.name == "alpine-3.20") | .name' 2>/dev/null)
if [ "$NAME" = "alpine-3.20" ]; then
    pass "JSON contains alpine-3.20 image entry"
else
    fail "JSON missing alpine-3.20: $JSON_OUTPUT"
fi

# -- Catalog file exists (copied during Docker build) ─────────────────

if docker exec "e2e-image-list" test -f /opt/syfrah/catalog.json 2>/dev/null; then
    CATALOG_IMAGES=$(docker exec "e2e-image-list" jq '.images | length' /opt/syfrah/catalog.json 2>/dev/null)
    if [ "$CATALOG_IMAGES" -gt 0 ]; then
        pass "catalog.json has $CATALOG_IMAGES image(s)"
    else
        fail "catalog.json has no images"
    fi
else
    fail "catalog.json not found in container"
fi

cleanup
summary
