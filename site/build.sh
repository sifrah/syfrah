#!/usr/bin/env bash
set -euo pipefail

# Build the Syfrah documentation site
# Converts markdown files from across the repo into static HTML

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SITE_DIR="$REPO_ROOT/site"
DIST="$REPO_ROOT/site/dist"
TEMPLATE="$SITE_DIR/template.html"

rm -rf "$DIST"
mkdir -p "$DIST/layers" "$DIST/operations" "$DIST/reference"

# Copy static assets
cp "$SITE_DIR/style.css" "$DIST/"

# Convert a markdown file to HTML using pandoc
# Usage: convert <input.md> <output.html> <root-prefix>
convert() {
    local input="$1"
    local output="$2"
    local root="$3"

    # Extract title from first H1, fallback to filename
    local title
    title=$(grep -m1 '^# ' "$input" | sed 's/^# //' || basename "$input" .md)

    pandoc "$input" \
        --from markdown \
        --to html5 \
        --template "$TEMPLATE" \
        --variable "title:$title" \
        --variable "root:$root" \
        --no-highlight \
        -o "$output"
}

echo "Building documentation site..."

# Overview
convert "$REPO_ROOT/docs/ARCHITECTURE.md" "$DIST/index.html" ""

# Layers (from README.md in each layer)
for layer in fabric forge compute storage overlay controlplane org iam products; do
    readme="$REPO_ROOT/layers/$layer/README.md"
    if [ -f "$readme" ]; then
        convert "$readme" "$DIST/layers/$layer.html" "../"
    fi
done

# Operations (from docs/)
convert "$REPO_ROOT/docs/cli.md" "$DIST/operations/cli.html" "../"
convert "$REPO_ROOT/docs/state-and-reconciliation.md" "$DIST/operations/state-and-reconciliation.html" "../"
convert "$REPO_ROOT/docs/zones-and-regions.md" "$DIST/operations/zones-and-regions.html" "../"

# Reference
convert "$REPO_ROOT/docs/repository.md" "$DIST/reference/repository.html" "../"

echo "Done. Output: $DIST/"
echo "Pages:"
find "$DIST" -name '*.html' | sort | while read -r f; do
    echo "  ${f#$DIST/}"
done
