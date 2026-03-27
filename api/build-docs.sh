#!/usr/bin/env bash
set -euo pipefail

# Build Redocly API documentation into a single static HTML file.
# Output lands in docs/dist/api/index.html so it can be served alongside
# the main Starlight documentation site.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="${ROOT_DIR}/docs/dist/api"

mkdir -p "$OUTPUT_DIR"

npx @redocly/cli build-docs "$SCRIPT_DIR/openapi.yaml" -o "$OUTPUT_DIR/index.html"

echo "API docs built -> $OUTPUT_DIR/index.html"
