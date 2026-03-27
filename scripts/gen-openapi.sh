#!/usr/bin/env bash
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Generate per-layer OpenAPI specs from proto
python3 "$REPO_ROOT/scripts/gen-openapi.py" "$REPO_ROOT"

echo "--- Merging layer specs ---"

LAYER_SPECS=()
for spec in "$REPO_ROOT"/layers/*/openapi.yaml; do
  [ -f "$spec" ] && LAYER_SPECS+=("$spec")
done

[ "${#LAYER_SPECS[@]}" -eq 0 ] && echo "ERROR: No specs" >&2 && exit 1

# Merge specs with clean tags
python3 - "${LAYER_SPECS[@]}" << 'PYMERGE'
import sys, yaml
from collections import OrderedDict

merged = {
    "openapi": "3.0.3",
    "info": {"title": "Syfrah API", "version": "1.0.0", "description": "API reference for all Syfrah layers."},
    "servers": [{"url": "https://{gateway}", "variables": {"gateway": {"default": "localhost:8443"}}}],
    "security": [{"BearerAuth": []}],
    "paths": {},
    "components": {"schemas": {}, "securitySchemes": {"BearerAuth": {"type": "http", "scheme": "bearer", "bearerFormat": "syf_key_*"}}},
    "tags": [],
    "x-tagGroups": []
}

tag_set = OrderedDict()

for spec_path in sys.argv[1:]:
    with open(spec_path) as f:
        spec = yaml.safe_load(f)
    
    # Extract layer name from tags
    layer_name = None
    for tag in spec.get("tags", []):
        name = tag["name"]
        layer_name = name
        if name not in tag_set:
            tag_set[name] = tag.get("description", f"{name} API")
    
    # Merge paths, rewriting tags to clean layer name
    for path, methods in spec.get("paths", {}).items():
        if path not in merged["paths"]:
            merged["paths"][path] = {}
        for method, details in methods.items():
            if isinstance(details, dict) and layer_name:
                details["tags"] = [layer_name]
            merged["paths"][path][method] = details
    
    # Merge schemas
    for name, schema in spec.get("components", {}).get("schemas", {}).items():
        merged["components"]["schemas"][name] = schema

# Build tags and groups
for name, desc in tag_set.items():
    merged["tags"].append({"name": name, "description": desc})
    merged["x-tagGroups"].append({"name": name, "tags": [name]})

with open("api/openapi.yaml", "w") as f:
    yaml.dump(merged, f, default_flow_style=False, sort_keys=False, allow_unicode=True)

print(f"Merged {len(sys.argv)-1} specs, {len(tag_set)} categories, {len(merged['paths'])} paths")
PYMERGE

echo "--- Building Scalar API docs ---"
mkdir -p "$REPO_ROOT/docs/dist/api"

# Generate Scalar HTML page
cat > "$REPO_ROOT/docs/dist/api/index.html" << 'HTML'
<!DOCTYPE html>
<html>
<head>
  <title>Syfrah API Reference</title>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
</head>
<body>
  <script id="api-reference" data-url="/syfrah/api/openapi.yaml"></script>
  <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
</body>
</html>
HTML

# Copy the generated spec alongside the HTML
cp "$REPO_ROOT/api/openapi.yaml" "$REPO_ROOT/docs/dist/api/openapi.yaml"

echo "Scalar API docs ready at docs/dist/api/"
