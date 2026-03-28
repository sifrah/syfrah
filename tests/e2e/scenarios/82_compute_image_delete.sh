#!/usr/bin/env bash
# Scenario: Image deletion
#
# Prerequisites:
#   - Docker image built with real images from syfrah-images catalog
#   - Compute CLI (syfrah compute image delete) must be implemented
#
# Verifies:
#   - An imported image can be deleted
#   - The deleted image no longer appears in list
#   - The raw file is removed from disk
#   - Deleting a non-existent image returns an error

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "-- Compute: Image Delete --"

create_network

start_node "e2e-image-delete" "172.20.0.10"
init_mesh "e2e-image-delete" "172.20.0.10" "image-delete"
sleep 2

# -- Import a test image to delete ────────────────────────────────────

info "Creating and importing disposable image"
docker exec "e2e-image-delete" dd if=/dev/zero of=/tmp/disposable.raw bs=1M count=4 2>/dev/null
import_image "e2e-image-delete" "/tmp/disposable.raw" "disposable"
sleep 1

assert_image_exists "e2e-image-delete" "disposable"

# -- Delete the image ─────────────────────────────────────────────────

info "Deleting disposable image"
OUTPUT=$(delete_image "e2e-image-delete" "disposable")

if echo "$OUTPUT" | grep -qi "Deleted\|disposable"; then
    pass "delete command succeeded"
else
    fail "delete command failed: $OUTPUT"
fi

# -- Verify image gone ────────────────────────────────────────────────

sleep 1
assert_image_gone "e2e-image-delete" "disposable"

LIST_OUTPUT=$(list_images "e2e-image-delete")
if echo "$LIST_OUTPUT" | grep -q "disposable"; then
    fail "disposable still in image list after delete"
else
    pass "disposable removed from image list"
fi

# -- Delete non-existent image ────────────────────────────────────────

info "Deleting non-existent image"
EXIT_CODE=0
OUTPUT=$(delete_image "e2e-image-delete" "ghost-image" 2>&1) || EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ] || echo "$OUTPUT" | grep -qi "not found\|error\|no such"; then
    pass "deleting non-existent image fails as expected"
else
    fail "deleting non-existent image did not fail: $OUTPUT"
fi

cleanup
summary
