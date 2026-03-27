#!/usr/bin/env bash
# gen-openapi.sh — Generate OpenAPI 3.0 specs from proto files.
#
# For each layer with a proto file, this script generates an openapi.yaml
# derived from the proto service definition and HTTP annotations.
# Finally it merges all layer specs into api/openapi.yaml via redocly join.
#
# Usage: bash scripts/gen-openapi.sh

set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

python3 "$REPO_ROOT/scripts/gen-openapi.py" "$REPO_ROOT"

echo "--- Merging layer specs into api/openapi.yaml ---"

LAYER_SPECS=()
for spec in "$REPO_ROOT"/layers/*/openapi.yaml; do
  [ -f "$spec" ] && LAYER_SPECS+=("$spec")
done

if [ "${#LAYER_SPECS[@]}" -eq 0 ]; then
  echo "ERROR: No layer openapi.yaml files found" >&2
  exit 1
fi

if [ "${#LAYER_SPECS[@]}" -eq 1 ]; then
  # redocly join needs at least 2 files; just copy the single spec
  cp "${LAYER_SPECS[0]}" "$REPO_ROOT/api/openapi.yaml"
else
  npx -y @redocly/cli join "${LAYER_SPECS[@]}" -o "$REPO_ROOT/api/openapi.yaml" --without-x-tag-groups
fi

echo "Generated api/openapi.yaml with ${#LAYER_SPECS[@]} layer(s)"
