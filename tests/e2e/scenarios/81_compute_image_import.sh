#!/usr/bin/env bash
# Scenario: Image import from local file
#
# Prerequisites:
#   - Docker image built with real images from syfrah-images catalog
#   - Compute CLI (syfrah compute image import) must be implemented
#
# Verifies:
#   - A raw disk file can be imported with a custom name
#   - The imported image appears in image list
#   - The imported image can be inspected

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "-- Compute: Image Import --"

create_network

start_node "e2e-image-import" "172.20.0.10"
init_mesh "e2e-image-import" "172.20.0.10" "image-import"
sleep 2

# -- Create a small test raw file to import ─────────────────────────

info "Creating test raw file"
docker exec "e2e-image-import" dd if=/dev/zero of=/tmp/test-disk.raw bs=1M count=8 2>/dev/null
if [ $? -eq 0 ]; then
    pass "created 8 MB test raw file"
else
    fail "failed to create test raw file"
fi

# -- Import the raw file ─────────────────────────────────────────────

info "Importing test-disk.raw as custom-os"
OUTPUT=$(import_image "e2e-image-import" "/tmp/test-disk.raw" "custom-os")

if echo "$OUTPUT" | grep -qi "Imported\|custom-os"; then
    pass "import command succeeded"
else
    fail "import command failed: $OUTPUT"
fi

# -- Verify imported image in list ────────────────────────────────────

sleep 1
LIST_OUTPUT=$(list_images "e2e-image-import")

if echo "$LIST_OUTPUT" | grep -q "custom-os"; then
    pass "custom-os appears in image list"
else
    fail "custom-os not in image list: $LIST_OUTPUT"
fi

# -- Verify imported image file on disk ───────────────────────────────

assert_image_exists "e2e-image-import" "custom-os"

# -- Verify import of duplicate name fails ────────────────────────────

info "Importing duplicate name (should fail)"
EXIT_CODE=0
OUTPUT=$(import_image "e2e-image-import" "/tmp/test-disk.raw" "custom-os" 2>&1) || EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ] || echo "$OUTPUT" | grep -qi "already exists\|error"; then
    pass "duplicate import rejected"
else
    fail "duplicate import not rejected: $OUTPUT"
fi

cleanup
summary
