#!/usr/bin/env bash
# sync.sh — Copy markdown files from the repo into docs/src/content/docs/
# and inject Starlight-compatible frontmatter where missing.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DOCS_DIR="$(cd "$(dirname "$0")" && pwd)/src/content/docs"

# Clean previous sync
rm -rf "$DOCS_DIR"
mkdir -p "$DOCS_DIR"

# inject_frontmatter <file>
# If the file lacks YAML frontmatter, extract the first H1 as title and prepend it.
inject_frontmatter() {
  local file="$1"
  local title
  title=$(grep -m1 '^# ' "$file" | sed 's/^# //')
  if [ -z "$title" ]; then
    title="$(basename "$file" .md)"
  fi
  # Escape quotes in title
  title="${title//\"/\\\"}"
  if head -1 "$file" | grep -q '^---'; then
    # Has frontmatter — check if title is present
    if ! sed -n '/^---$/,/^---$/p' "$file" | grep -q '^title:'; then
      # Inject title into existing frontmatter (after first ---)
      local tmp
      tmp=$(mktemp)
      sed "0,/^---$/{s/^---$/---\ntitle: \"$title\"/}" "$file" > "$tmp"
      mv "$tmp" "$file"
    fi
    return
  fi
  local tmp
  tmp=$(mktemp)
  printf -- '---\ntitle: "%s"\n---\n\n' "$title" > "$tmp"
  cat "$file" >> "$tmp"
  mv "$tmp" "$file"
}

# copy_file <src> <dest_relative>
copy_file() {
  local src="$1"
  local dest="$DOCS_DIR/$2"
  mkdir -p "$(dirname "$dest")"
  cp "$src" "$dest"
  inject_frontmatter "$dest"
}

# --- Handbook ---
for f in "$REPO_ROOT"/handbook/*.md; do
  name=$(basename "$f" | tr '[:upper:]' '[:lower:]')
  copy_file "$f" "handbook/$name"
done

# --- Layers ---
for dir in "$REPO_ROOT"/layers/*/; do
  layer=$(basename "$dir")
  if [ -f "$dir/README.md" ]; then
    copy_file "$dir/README.md" "layers/$layer.md"
  fi
done

# --- API Reference ---
for f in "$REPO_ROOT"/api/proto/syfrah/v1/*.md; do
  name=$(basename "$f")
  copy_file "$f" "api/$name"
done

# --- Dev ---
for f in "$REPO_ROOT"/dev/*.md; do
  name=$(basename "$f" | tr '[:upper:]' '[:lower:]')
  copy_file "$f" "dev/$name"
done

# --- Benchmarks ---
for f in "$REPO_ROOT"/benchmarks/*.md; do
  name=$(basename "$f" | tr '[:upper:]' '[:lower:]')
  copy_file "$f" "benchmarks/$name"
done

# --- Post-release audits ---
for f in "$REPO_ROOT"/post_release_audit/*.md; do
  name=$(basename "$f" | tr '[:upper:]' '[:lower:]')
  copy_file "$f" "audits/$name"
done

# --- Landing page ---
cat > "$DOCS_DIR/index.mdx" <<'EOF'
---
title: Syfrah Documentation
description: Open-source control plane to transform dedicated servers into a programmable cloud.
template: splash
hero:
  tagline: Turn rented servers into a programmable cloud.
  actions:
    - text: Read the Architecture
      link: /syfrah/handbook/architecture/
      icon: right-arrow
    - text: View on GitHub
      link: https://github.com/sacha-ops/syfrah
      variant: minimal
      icon: external
---
EOF

echo "Synced docs into $DOCS_DIR"
