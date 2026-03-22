#!/usr/bin/env bash
set -euo pipefail

# Sync layer READMEs and cross-cutting docs into Next.js MDX pages.
# This script is the bridge between the source-of-truth markdown files
# and the documentation site.
#
# Usage: ./scripts/sync-docs.sh

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$REPO_ROOT/documentation/src/app"

# ── Helpers ──────────────────────────────────────────────────

# Escape characters that MDX would interpret as JSX:
#   < followed by a digit → &lt;
#   {word} patterns outside code blocks → &#123;word&#125;
escape_mdx() {
    sed \
        -e 's/<\([0-9]\)/\&lt;\1/g' \
        -e 's/{\([a-z_/][a-z_/.-]*\)}/\&#123;\1\&#125;/g'
}

# Extract the first H1 title from a markdown file
extract_title() {
    grep -m1 '^# ' "$1" | sed 's/^# //'
}

# Generate an MDX page file
# Args: <source.md> <output_dir> <title> <description>
generate_page() {
    local src="$1"
    local outdir="$2"
    local title="$3"
    local desc="$4"

    mkdir -p "$outdir"

    # Content = everything after the first H1 line
    local content
    content=$(tail -n +2 "$src" | escape_mdx)

    local rel_src="${src#$REPO_ROOT/}"

    cat > "$outdir/page.mdx" << MDXEOF
{/* AUTO-GENERATED from ${rel_src} — do not edit */}

export const metadata = {
  title: '${title}',
  description: '${desc}',
}

# ${title}

${content}
MDXEOF
}

echo "Syncing documentation..."

# ── Homepage (ARCHITECTURE.md) ───────────────────────────────

echo "  → index (ARCHITECTURE.md)"
mkdir -p "$APP_DIR"
content=$(tail -n +2 "$REPO_ROOT/docs/ARCHITECTURE.md" | escape_mdx)
cat > "$APP_DIR/page.mdx" << MDXEOF
{/* AUTO-GENERATED from docs/ARCHITECTURE.md — do not edit */}

export const metadata = {
  title: 'Architecture',
  description: 'Syfrah global architecture overview',
}

# Architecture

${content}
MDXEOF

# ── Layer pages ──────────────────────────────────────────────

declare -a LAYERS=(fabric forge compute storage overlay controlplane org iam products)

declare -a LAYER_TITLES=(
    "Fabric"
    "Forge"
    "Compute"
    "Storage"
    "Overlay"
    "Control Plane"
    "Organization Model"
    "IAM"
    "Cloud Products"
)

declare -a LAYER_DESCS=(
    "WireGuard mesh between all nodes"
    "Per-node REST API and debug interface"
    "Firecracker microVM compute layer"
    "ZeroFS and S3-backed block storage"
    "VXLAN, VPC, security groups, private DNS"
    "Raft consensus and gossip protocol"
    "Organization, project, and environment model"
    "Identity and access management"
    "Cloud product orchestration model"
)

for i in "${!LAYERS[@]}"; do
    layer="${LAYERS[$i]}"
    title="${LAYER_TITLES[$i]}"
    desc="${LAYER_DESCS[$i]}"
    readme="$REPO_ROOT/layers/$layer/README.md"

    if [ -f "$readme" ]; then
        echo "  → layers/$layer"
        generate_page "$readme" "$APP_DIR/$layer" "$title" "$desc"
    else
        echo "  ! layers/$layer/README.md not found, skipping"
    fi
done

# ── Cross-cutting docs ───────────────────────────────────────

declare -a DOCS=(cli state-and-reconciliation zones-and-regions repository)

declare -a DOC_TITLES=(
    "CLI"
    "State and Reconciliation"
    "Zones and Regions"
    "Repository Structure"
)

declare -a DOC_DESCS=(
    "CLI command reference"
    "Source of truth, reconciliation loop, and resource phases"
    "Logical topology with regions and availability zones"
    "Repository structure and conventions"
)

for i in "${!DOCS[@]}"; do
    doc="${DOCS[$i]}"
    title="${DOC_TITLES[$i]}"
    desc="${DOC_DESCS[$i]}"
    src="$REPO_ROOT/docs/$doc.md"

    if [ -f "$src" ]; then
        echo "  → docs/$doc"
        generate_page "$src" "$APP_DIR/$doc" "$title" "$desc"
    else
        echo "  ! docs/$doc.md not found, skipping"
    fi
done

echo "Done. $(find "$APP_DIR" -name 'page.mdx' | wc -l | tr -d ' ') pages synced."
