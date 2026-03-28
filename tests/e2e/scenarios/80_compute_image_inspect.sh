#!/usr/bin/env bash
# Scenario: Image inspection with real catalog images
#
# Prerequisites:
#   - Docker image built with real images from syfrah-images catalog
#   - Compute CLI (syfrah compute image inspect) must be implemented
#
# Verifies:
#   - Inspecting a known image returns metadata
#   - Metadata contains expected fields (name, arch, format)
#   - Inspecting a non-existent image returns an error

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "-- Compute: Image Inspect (real images) --"

create_network

start_node "e2e-image-inspect" "172.20.0.10"
init_mesh "e2e-image-inspect" "172.20.0.10" "image-inspect"
sleep 2

# -- Inspect existing image ───────────────────────────────────────────

info "Inspecting alpine-3.20"
INSPECT_OUTPUT=$(inspect_image "e2e-image-inspect" "alpine-3.20")

if echo "$INSPECT_OUTPUT" | jq -e '.' >/dev/null 2>&1; then
    pass "inspect output is valid JSON"
else
    fail "inspect output is not valid JSON: $INSPECT_OUTPUT"
fi

NAME=$(echo "$INSPECT_OUTPUT" | jq -r '.name // empty' 2>/dev/null)
if [ "$NAME" = "alpine-3.20" ]; then
    pass "inspect shows correct name"
else
    fail "inspect name: '$NAME' (expected alpine-3.20)"
fi

FORMAT=$(echo "$INSPECT_OUTPUT" | jq -r '.format // empty' 2>/dev/null)
if [ "$FORMAT" = "raw" ]; then
    pass "inspect shows format=raw"
else
    fail "inspect format: '$FORMAT' (expected raw)"
fi

SIZE_MB=$(echo "$INSPECT_OUTPUT" | jq -r '.size_mb // 0' 2>/dev/null)
if [ "$SIZE_MB" -gt 0 ]; then
    pass "inspect shows non-zero size (${SIZE_MB} MB)"
else
    fail "inspect size_mb is 0 or missing"
fi

# -- Inspect non-existent image ──────────────────────────────────────

info "Inspecting non-existent image"
EXIT_CODE=0
OUTPUT=$(inspect_image "e2e-image-inspect" "no-such-image" 2>&1) || EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ] || echo "$OUTPUT" | grep -qi "not found\|error\|no such"; then
    pass "inspecting non-existent image fails as expected"
else
    fail "inspecting non-existent image did not fail: $OUTPUT"
fi

cleanup
summary
