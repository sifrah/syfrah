#!/usr/bin/env bash
set -euo pipefail

# Auto-sync all .md files into Next.js MDX pages.
# Recursively scans configured directories — zero manual config needed.
#
# Usage: ./scripts/sync-docs.sh

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$REPO_ROOT/documentation/src/app"
NAV_FILE="$REPO_ROOT/documentation/src/navigation.json"

# Directories to scan for .md files
SCAN_DIRS=(handbook layers dev benchmarks post_release_audit sdk api)

# Files and directories to exclude
EXCLUDE_DIRS=(node_modules target .git documentation .claude)
EXCLUDE_FILES=(CHANGELOG.md CODE_OF_CONDUCT.md SECURITY.md LICENSE .env)

# ── Helpers ──────────────────────────────────────────────────

escape_mdx() {
    sed \
        -e 's/<\([0-9]\)/\&lt;\1/g' \
        -e 's/{\([a-z_/][a-z_/.-]*\)}/\&#123;\1\&#125;/g'
}

# Extract H1 title from a markdown file, fallback to provided default
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

# Get last-updated info from git history for a source file
# Returns: "YYYY-MM-DD (abcdef0)" or "Not yet committed"
get_last_updated() {
    local file="$1"
    local git_info
    git_info=$(git -C "$REPO_ROOT" log -1 --format='%ai %h' -- "$file" 2>/dev/null || true)
    if [ -n "$git_info" ]; then
        local date hash
        date=$(echo "$git_info" | awk '{print $1}')
        hash=$(echo "$git_info" | awk '{print $4}')
        echo "${date} (${hash})"
    else
        echo "Not yet committed"
    fi
}

# Detect implementation status of a layer
# Returns: "implemented", "stub", or "planned"
detect_layer_status() {
    local layer_name="$1"
    local layer_dir="$REPO_ROOT/layers/$layer_name"
    local lib_rs="$layer_dir/src/lib.rs"
    local api_rs="$layer_dir/src/api.rs"

    if [ -f "$lib_rs" ]; then
        local line_count
        line_count=$(wc -l < "$lib_rs")
        if [ "$line_count" -gt 100 ]; then
            echo "implemented"
            return
        fi
    fi

    if [ -f "$api_rs" ]; then
        echo "stub"
        return
    fi

    echo "planned"
}

# Return the badge string for a layer status
status_badge() {
    case "$1" in
        implemented) echo "🟢 Implemented" ;;
        stub)        echo "🔵 Stub" ;;
        planned)     echo "⚪ Planned" ;;
        *)           echo "⚪ Planned" ;;
    esac
}

# Humanize a directory/file name: underscores/hyphens to spaces, title case
humanize() {
    echo "$1" | sed 's/[_-]/ /g' | sed 's/\b\(.\)/\u\1/g' \
        | sed 's/\bSdk\b/SDK/g; s/\bApi\b/API/g; s/\bCli\b/CLI/g; s/\bCi\b/CI/g'
}

# Generate an MDX page from a markdown file
# Args: src outdir title desc rel_src [badge]
generate_page() {
    local src="$1"
    local outdir="$2"
    local title="$3"
    local desc="$4"
    local rel_src="$5"
    local badge="${6:-}"

    mkdir -p "$outdir"

    local content
    content=$(tail -n +2 "$src" | escape_mdx)

    local last_updated
    last_updated=$(get_last_updated "$rel_src")

    local badge_line=""
    if [ -n "$badge" ]; then
        badge_line="<p className=\"inline-block rounded-full bg-zinc-100 px-3 py-1 text-sm font-medium dark:bg-zinc-800\">${badge}</p>
"
    fi

    cat > "$outdir/page.mdx" << MDXEOF
{/* AUTO-GENERATED from ${rel_src} — do not edit */}

export const metadata = {
  title: '${title}',
  description: '${desc}',
}

# ${title}

${badge_line}<p className="text-sm text-gray-500">Last updated: ${last_updated}</p>

${content}
MDXEOF
}

# ── Clean previous generated pages ──────────────────────────

# Remove all auto-generated page.mdx except layout/providers/not-found
find "$APP_DIR" -name 'page.mdx' -type f -exec grep -l 'AUTO-GENERATED' {} \; | while read -r f; do
    rm "$f"
done

# Remove empty directories left behind
find "$APP_DIR" -mindepth 1 -type d -empty -delete 2>/dev/null || true

echo "Syncing documentation..."

# ── Build find command with exclusions ───────────────────────

build_find_args() {
    local dir="$1"
    local args=("$REPO_ROOT/$dir")

    # Add directory exclusions
    local first_excl=true
    args+=("(")
    for excl in "${EXCLUDE_DIRS[@]}"; do
        if [ "$first_excl" = true ]; then
            first_excl=false
        else
            args+=("-o")
        fi
        args+=("-name" "$excl" "-type" "d")
    done
    args+=(")" "-prune" "-o")

    # Match .md files, exclude specific filenames
    args+=("-name" "*.md" "-type" "f")
    for excl_f in "${EXCLUDE_FILES[@]}"; do
        args+=("!" "-name" "$excl_f")
    done
    args+=("-print")

    find "${args[@]}" 2>/dev/null | sort
}

# ── Special case: ARCHITECTURE.md → homepage ─────────────────

echo "  homepage (ARCHITECTURE.md)"
arch_src="$REPO_ROOT/handbook/ARCHITECTURE.md"
if [ -f "$arch_src" ]; then
    content=$(tail -n +2 "$arch_src" | escape_mdx)
    home_updated=$(get_last_updated "handbook/ARCHITECTURE.md")
    cat > "$APP_DIR/page.mdx" << MDXEOF
{/* AUTO-GENERATED from handbook/ARCHITECTURE.md — do not edit */}

export const metadata = {
  title: 'Architecture',
  description: 'Syfrah global architecture overview',
}

# Architecture

<p className="text-sm text-gray-500">Last updated: ${home_updated}</p>

${content}
MDXEOF
fi

# ── Collect all .md files and generate pages ─────────────────

# Associative arrays to track the nav tree
# nav_groups[scan_dir] = JSON array string of top-level links
declare -A nav_groups

for scan_dir in "${SCAN_DIRS[@]}"; do
    [ -d "$REPO_ROOT/$scan_dir" ] || continue

    # Collect all .md files in this scan directory
    mapfile -t md_files < <(build_find_args "$scan_dir")
    [ ${#md_files[@]} -gt 0 ] || continue

    # Skip ARCHITECTURE.md (already handled as homepage)
    filtered_files=()
    for f in "${md_files[@]}"; do
        if [ "$f" = "$REPO_ROOT/handbook/ARCHITECTURE.md" ]; then
            continue
        fi
        filtered_files+=("$f")
    done
    [ ${#filtered_files[@]} -gt 0 ] || continue

    # Build page for each file and collect nav entries
    # We need a tree: top-level dirs become nav links, subdirs become children

    # Associative array: dir_path -> list of {title, href} entries
    declare -A dir_children
    declare -A dir_readmes
    declare -a dir_order=()

    for md_file in "${filtered_files[@]}"; do
        rel_path="${md_file#$REPO_ROOT/}"            # e.g. handbook/cli.md or layers/fabric/README.md
        rel_inside="${md_file#$REPO_ROOT/$scan_dir/}" # e.g. cli.md or fabric/README.md

        filename="$(basename "$rel_inside")"
        dir_inside="$(dirname "$rel_inside")"        # e.g. . or fabric or sdk/go

        # Compute the URL path for this page
        if [ "$filename" = "README.md" ]; then
            # README.md represents the directory itself
            if [ "$dir_inside" = "." ]; then
                url_path="/$scan_dir"
            else
                url_path="/$scan_dir/$dir_inside"
            fi
        else
            # Regular .md file: strip .md extension for URL
            name_no_ext="${filename%.md}"
            if [ "$dir_inside" = "." ]; then
                # handbook files live at root (e.g. /cli), others keep prefix
                if [ "$scan_dir" = "handbook" ]; then
                    url_path="/$name_no_ext"
                else
                    url_path="/$scan_dir/$name_no_ext"
                fi
            else
                url_path="/$scan_dir/$dir_inside/$name_no_ext"
            fi
        fi

        # Compute output directory
        local_path="${url_path#/}"
        outdir="$APP_DIR/$local_path"

        # Compute title
        fallback="$(humanize "${filename%.md}")"
        if [ "$filename" = "README.md" ]; then
            if [ "$dir_inside" = "." ]; then
                fallback="$(humanize "$scan_dir")"
            else
                fallback="$(humanize "$(basename "$dir_inside")")"
            fi
        fi
        title=$(extract_title "$md_file" "$fallback")

        # Detect layer status badge if this is a layer page
        page_badge=""
        layer_status=""
        if [ "$scan_dir" = "layers" ]; then
            layer_name=""
            if [ "$dir_inside" = "." ] && [ "$filename" = "README.md" ]; then
                # This shouldn't happen (layers/README.md at root)
                layer_name=""
            elif [ "$dir_inside" != "." ]; then
                layer_name="$(echo "$dir_inside" | cut -d/ -f1)"
            else
                layer_name=""
            fi
            if [ -n "$layer_name" ]; then
                layer_status=$(detect_layer_status "$layer_name")
                page_badge=$(status_badge "$layer_status")
            fi
        fi

        echo "  $local_path"
        generate_page "$md_file" "$outdir" "$title" "$title" "$rel_path" "$page_badge"

        # Track for navigation
        # Determine the parent nav node for this file
        if [ "$filename" = "README.md" ]; then
            # This file IS the page for its directory
            dir_readmes["$dir_inside"]="$title|$url_path|$layer_status"
        else
            # Regular file: its parent is dir_inside
            name_no_ext="${filename%.md}"
            key="$dir_inside"
            existing="${dir_children[$key]:-}"
            entry="{\"title\":\"$title\",\"href\":\"$url_path\"}"
            if [ -z "$existing" ]; then
                dir_children["$key"]="$entry"
            else
                dir_children["$key"]="$existing,$entry"
            fi
        fi

        # Track directory ordering
        if [ "$dir_inside" != "." ]; then
            top_dir="$(echo "$dir_inside" | cut -d/ -f1)"
        else
            top_dir="."
        fi
    done

    # Build the navigation JSON for this scan_dir
    # Strategy: collect top-level items (depth=1 dirs and root-level .md files)

    links_json=""

    # First: if there's a README at the root of scan_dir, it becomes the group page
    # but we skip handbook root since there's no handbook/README.md typically

    # Collect all unique top-level directories that have content
    declare -A seen_top_dirs
    top_level_links=""

    for md_file in "${filtered_files[@]}"; do
        rel_inside="${md_file#$REPO_ROOT/$scan_dir/}"
        filename="$(basename "$rel_inside")"
        dir_inside="$(dirname "$rel_inside")"

        if [ "$dir_inside" = "." ]; then
            # Root-level file in this scan_dir
            if [ "$filename" = "README.md" ]; then
                # scan_dir's own page — add as first link
                info="${dir_readmes[.]:-}"
                if [ -n "$info" ]; then
                    t="${info%%|*}"
                    rest="${info#*|}"
                    h="${rest%%|*}"
                    s="${rest##*|}"
                    if [ -n "$s" ] && [ "$scan_dir" = "layers" ]; then
                        entry="{\"title\":\"$t\",\"href\":\"$h\",\"status\":\"$s\"}"
                    else
                        entry="{\"title\":\"$t\",\"href\":\"$h\"}"
                    fi
                    if [ -z "$top_level_links" ]; then
                        top_level_links="$entry"
                    else
                        top_level_links="$top_level_links,$entry"
                    fi
                fi
            else
                # Regular .md at root of scan_dir
                name_no_ext="${filename%.md}"
                fallback="$(humanize "$name_no_ext")"
                title=$(extract_title "$md_file" "$fallback")
                if [ "$scan_dir" = "handbook" ]; then
                    url_path="/$name_no_ext"
                else
                    url_path="/$scan_dir/$name_no_ext"
                fi
                entry="{\"title\":\"$title\",\"href\":\"$url_path\"}"
                if [ -z "$top_level_links" ]; then
                    top_level_links="$entry"
                else
                    top_level_links="$top_level_links,$entry"
                fi
            fi
        else
            # File in a subdirectory
            top_dir="$(echo "$dir_inside" | cut -d/ -f1)"
            if [ -z "${seen_top_dirs[$top_dir]:-}" ]; then
                seen_top_dirs["$top_dir"]=1

                # Build this top-level dir's nav entry with children
                dir_info="${dir_readmes[$top_dir]:-}"
                nav_status=""
                if [ -n "$dir_info" ]; then
                    t="${dir_info%%|*}"
                    rest="${dir_info#*|}"
                    h="${rest%%|*}"
                    nav_status="${rest##*|}"
                else
                    t="$(humanize "$top_dir")"
                    h="/$scan_dir/$top_dir"
                fi

                # Collect children: sub-files and sub-dirs
                children=""
                child_entries="${dir_children[$top_dir]:-}"

                # Also look for sub-directory READMEs (depth > 1)
                for key in "${!dir_readmes[@]}"; do
                    if [[ "$key" == "$top_dir/"* ]]; then
                        sub_info="${dir_readmes[$key]}"
                        st="${sub_info%%|*}"
                        sub_rest="${sub_info#*|}"
                        sh="${sub_rest%%|*}"
                        sub_entry="{\"title\":\"$st\",\"href\":\"$sh\"}"
                        if [ -z "$children" ]; then
                            children="$sub_entry"
                        else
                            children="$children,$sub_entry"
                        fi
                    fi
                done

                # Add non-README children
                if [ -n "$child_entries" ]; then
                    if [ -z "$children" ]; then
                        children="$child_entries"
                    else
                        children="$children,$child_entries"
                    fi
                fi

                # Build status JSON fragment for layers
                status_fragment=""
                if [ -n "$nav_status" ] && [ "$scan_dir" = "layers" ]; then
                    status_fragment=",\"status\":\"$nav_status\""
                fi

                if [ -n "$children" ]; then
                    entry="{\"title\":\"$t\",\"href\":\"$h\"${status_fragment},\"children\":[$children]}"
                else
                    entry="{\"title\":\"$t\",\"href\":\"$h\"${status_fragment}}"
                fi

                if [ -z "$top_level_links" ]; then
                    top_level_links="$entry"
                else
                    top_level_links="$top_level_links,$entry"
                fi
            fi
        fi
    done

    if [ -n "$top_level_links" ]; then
        nav_groups["$scan_dir"]="[$top_level_links]"
    fi

    # Clean up per-scan_dir associative arrays
    unset dir_children
    unset dir_readmes
    unset seen_top_dirs
    declare -A dir_children
    declare -A dir_readmes
    declare -A seen_top_dirs
done

# ── Write navigation.json ────────────────────────────────────

# Map scan directories to navigation group names
# The Navigation.tsx expects: overview, layers, operations, reference
# We map: handbook → split into overview + operations + reference (legacy compat)
#          layers → layers
#          everything else → their humanized name

# Build the overview group (Architecture homepage)
overview_json='[{"title":"Architecture","href":"/"}]'

# Layers come from nav_groups[layers]
layers_json="${nav_groups[layers]:-[]}"

# All handbook pages go into a single "Handbook" group
handbook_json="${nav_groups[handbook]:-[]}"

# Build remaining groups
other_groups=""
for scan_dir in "${SCAN_DIRS[@]}"; do
    case "$scan_dir" in
        handbook|layers) continue ;;
    esac
    group_data="${nav_groups[$scan_dir]:-}"
    if [ -n "$group_data" ] && [ "$group_data" != "[]" ]; then
        group_title="$(humanize "$scan_dir")"
        if [ -n "$other_groups" ]; then
            other_groups="$other_groups,"
        fi
        other_groups="$other_groups\"$scan_dir\":{\"title\":\"$group_title\",\"links\":$group_data}"
    fi
done

cat > "$NAV_FILE" << NAVEOF
{
  "overview": $overview_json,
  "layers": $layers_json,
  "handbook": $handbook_json,
  "extra": {${other_groups}}
}
NAVEOF

echo "  navigation.json"

page_count=$(find "$APP_DIR" -name 'page.mdx' | wc -l | tr -d ' ')
echo "Done. $page_count pages synced."
