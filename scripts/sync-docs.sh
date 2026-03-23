#!/usr/bin/env bash
set -euo pipefail

# Sync layer READMEs and cross-cutting docs into Next.js MDX pages.
# Scans layers/ recursively — subdirectories become sub-pages and sub-menus.
#
# Usage: ./scripts/sync-docs.sh

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$REPO_ROOT/documentation/src/app"
NAV_FILE="$REPO_ROOT/documentation/src/navigation.json"

# ── Helpers ──────────────────────────────────────────────────

escape_mdx() {
    sed \
        -e 's/<\([0-9]\)/\&lt;\1/g' \
        -e 's/{\([a-z_/][a-z_/.-]*\)}/\&#123;\1\&#125;/g'
}

# Extract H1 title from a markdown file, fallback to directory name
extract_title() {
    local file="$1"
    local fallback="$2"
    local title
    title=$(grep -m1 '^# ' "$file" 2>/dev/null | sed 's/^# //')
    if [ -z "$title" ]; then
        echo "$fallback"
    else
        echo "$title"
    fi
}

# Generate an MDX page from a markdown file
# Args: <source.md> <output_dir> <title> <description> <relative_source>
generate_page() {
    local src="$1"
    local outdir="$2"
    local title="$3"
    local desc="$4"
    local rel_src="$5"

    mkdir -p "$outdir"

    local content
    content=$(tail -n +2 "$src" | escape_mdx)

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

# ── Collect navigation data ──────────────────────────────────

# We build a JSON navigation file that Navigation.tsx imports.
# Start with empty groups.

NAV_OVERVIEW='[]'
NAV_LAYERS='{}'
NAV_OPS='[]'
NAV_REF='[]'

# ── Homepage (ARCHITECTURE.md) ───────────────────────────────

echo "  → index (ARCHITECTURE.md)"
src="$REPO_ROOT/handbook/ARCHITECTURE.md"
content=$(tail -n +2 "$src" | escape_mdx)
cat > "$APP_DIR/page.mdx" << MDXEOF
{/* AUTO-GENERATED from handbook/ARCHITECTURE.md — do not edit */}

export const metadata = {
  title: 'Architecture',
  description: 'Syfrah global architecture overview',
}

# Architecture

${content}
MDXEOF

NAV_OVERVIEW='[{"title":"Architecture","href":"/"}]'

# ── Layer pages (recursive scan) ─────────────────────────────

# Scan layers/ for all README.md files
# layers/fabric/README.md         → /fabric
# layers/fabric/security/README.md → /fabric/security

for readme in $(find "$REPO_ROOT/layers" -name "README.md" -type f | sort); do
    # Get the path relative to layers/
    rel="${readme#$REPO_ROOT/layers/}"          # e.g. fabric/README.md or fabric/security/README.md
    dir="$(dirname "$rel")"                      # e.g. fabric or fabric/security
    layer="$(echo "$dir" | cut -d/ -f1)"         # e.g. fabric (top-level layer name)
    rel_src="layers/$rel"

    # Compute the URL path
    url_path="/$dir"                             # e.g. /fabric or /fabric/security

    # Compute title from the README
    fallback_name="$(basename "$dir" | sed 's/-/ /g; s/\b\(.\)/\u\1/g')"
    title=$(extract_title "$readme" "$fallback_name")

    # Create the page
    outdir="$APP_DIR/$dir"
    echo "  → $dir"
    generate_page "$readme" "$outdir" "$title" "$title" "$rel_src"
done

# ── Build layer navigation tree ──────────────────────────────

# For each layer, build a JSON object: { title, href, children: [...] }
# We use a simple approach: list all pages per top-level layer

NAV_LAYERS_JSON="["
first_layer=true

for layer_dir in "$REPO_ROOT"/layers/*/; do
    layer="$(basename "$layer_dir")"
    layer_readme="$layer_dir/README.md"
    [ -f "$layer_readme" ] || continue

    layer_title=$(extract_title "$layer_readme" "$layer")
    layer_href="/$layer"

    # Find children (subdirectories with README.md)
    children_json="["
    first_child=true

    for child_readme in $(find "$layer_dir" -mindepth 2 -name "README.md" -type f | sort); do
        child_rel="${child_readme#$layer_dir}"    # e.g. security/README.md
        child_dir="$(dirname "$child_rel")"        # e.g. security
        child_href="/$layer/$child_dir"

        child_fallback="$(basename "$child_dir" | sed 's/-/ /g; s/\b\(.\)/\u\1/g')"
        child_title=$(extract_title "$child_readme" "$child_fallback")

        if [ "$first_child" = true ]; then
            first_child=false
        else
            children_json="$children_json,"
        fi
        children_json="$children_json{\"title\":\"$child_title\",\"href\":\"$child_href\"}"
    done

    children_json="$children_json]"

    if [ "$first_layer" = true ]; then
        first_layer=false
    else
        NAV_LAYERS_JSON="$NAV_LAYERS_JSON,"
    fi
    NAV_LAYERS_JSON="$NAV_LAYERS_JSON{\"title\":\"$layer_title\",\"href\":\"$layer_href\",\"children\":$children_json}"
done

NAV_LAYERS_JSON="$NAV_LAYERS_JSON]"

# ── Cross-cutting docs ───────────────────────────────────────

# These are manually mapped since they have custom titles
declare -a DOCS=(cli state-and-reconciliation zones-and-regions)
declare -a DOC_TITLES=("CLI" "State and Reconciliation" "Zones and Regions")

NAV_OPS_JSON="["
first_doc=true

for i in "${!DOCS[@]}"; do
    doc="${DOCS[$i]}"
    title="${DOC_TITLES[$i]}"
    src="$REPO_ROOT/handbook/$doc.md"

    if [ -f "$src" ]; then
        echo "  → handbook/$doc"
        generate_page "$src" "$APP_DIR/$doc" "$title" "$title" "handbook/$doc.md"

        if [ "$first_doc" = true ]; then
            first_doc=false
        else
            NAV_OPS_JSON="$NAV_OPS_JSON,"
        fi
        NAV_OPS_JSON="$NAV_OPS_JSON{\"title\":\"$title\",\"href\":\"/$doc\"}"
    fi
done

NAV_OPS_JSON="$NAV_OPS_JSON]"

# Reference docs
declare -a REFS=(repository documentation-strategy ci testing state-store)
declare -a REF_TITLES=("Repository Structure" "Documentation Strategy" "CI/CD" "Testing" "State Store")

NAV_REF_JSON="["
first_ref=true

for i in "${!REFS[@]}"; do
    doc="${REFS[$i]}"
    title="${REF_TITLES[$i]}"
    src="$REPO_ROOT/handbook/$doc.md"

    if [ -f "$src" ]; then
        echo "  → handbook/$doc"
        generate_page "$src" "$APP_DIR/$doc" "$title" "$title" "handbook/$doc.md"

        if [ "$first_ref" = true ]; then
            first_ref=false
        else
            NAV_REF_JSON="$NAV_REF_JSON,"
        fi
        NAV_REF_JSON="$NAV_REF_JSON{\"title\":\"$title\",\"href\":\"/$doc\"}"
    fi
done

NAV_REF_JSON="$NAV_REF_JSON]"

# ── Write navigation.json ────────────────────────────────────

cat > "$NAV_FILE" << NAVEOF
{
  "overview": $NAV_OVERVIEW,
  "layers": $NAV_LAYERS_JSON,
  "operations": $NAV_OPS_JSON,
  "reference": $NAV_REF_JSON
}
NAVEOF

echo "  → navigation.json"

page_count=$(find "$APP_DIR" -name 'page.mdx' | wc -l | tr -d ' ')
echo "Done. $page_count pages synced."
