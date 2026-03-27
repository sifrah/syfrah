#!/usr/bin/env python3
"""Generate OpenAPI 3.0 YAML from proto files with google.api.http annotations.

Parses each layers/{name}/proto/{name}.proto, extracts the service RPCs,
HTTP annotations, and message definitions, then writes an OpenAPI 3.0 spec
to layers/{name}/openapi.yaml.

For stub layers (those whose RPCs only have GetStatus), the spec shows a
single GET /v1/{layer}/status endpoint returning 501 Not Implemented.
"""

import os
import re
import sys
import textwrap
from collections import OrderedDict
from pathlib import Path


def parse_proto(proto_path: str) -> dict:
    """Parse a proto file and extract service, RPCs, messages, and enums."""
    with open(proto_path) as f:
        content = f.read()

    result = {
        "service_name": "",
        "service_comment": "",
        "version": "",
        "rpcs": [],
        "messages": {},
        "enums": {},
    }

    # Extract version from package declaration (e.g. package syfrah.fabric.v1;)
    pkg_match = re.search(r"^package\s+syfrah\.\w+\.(v\d+)\s*;", content, re.MULTILINE)
    if pkg_match:
        result["version"] = pkg_match.group(1)

    # Extract service name
    svc_match = re.search(r"service\s+(\w+)\s*\{", content)
    if svc_match:
        result["service_name"] = svc_match.group(1)

    # Extract service-level comment
    svc_comment = re.search(r"((?://[^\n]*\n)+)\s*service\s+\w+", content)
    if svc_comment:
        lines = svc_comment.group(1).strip().split("\n")
        result["service_comment"] = " ".join(
            line.lstrip("/").strip() for line in lines
        )

    # Extract RPCs with HTTP annotations
    rpc_pattern = re.compile(
        r"((?://[^\n]*\n\s*)*)"  # leading comments
        r"rpc\s+(\w+)\s*\(\s*(\w+)\s*\)\s*returns\s*\(\s*(\w+)\s*\)\s*"
        r"(?:\{[^}]*option\s*\(google\.api\.http\)\s*=\s*\{([^}]*)\}[^}]*\})?",
        re.MULTILINE,
    )
    for m in rpc_pattern.finditer(content):
        comment_block = m.group(1) or ""
        comment_lines = [
            l.strip().lstrip("/").strip()
            for l in comment_block.strip().split("\n")
            if l.strip().startswith("//") and not l.strip().startswith("// ---")
        ]
        comment = " ".join(cl for cl in comment_lines if cl)

        http_block = m.group(5) or ""
        http_method = ""
        http_path = ""
        has_body = False

        for token in ["get", "post", "put", "patch", "delete"]:
            path_match = re.search(
                rf'{token}\s*:\s*"([^"]+)"', http_block
            )
            if path_match:
                http_method = token.upper()
                http_path = path_match.group(1)
                break

        if 'body:' in http_block:
            has_body = True

        result["rpcs"].append(
            {
                "name": m.group(2),
                "request": m.group(3),
                "response": m.group(4),
                "comment": comment,
                "http_method": http_method,
                "http_path": http_path,
                "has_body": has_body,
            }
        )

    # Extract messages with their fields
    msg_pattern = re.compile(
        r"message\s+(\w+)\s*\{(.*?)\}", re.DOTALL
    )
    for m in msg_pattern.finditer(content):
        msg_name = m.group(1)
        body = m.group(2)
        fields = []

        field_pattern = re.compile(
            r"((?://[^\n]*\n\s*)*)"  # leading comments
            r"(optional\s+|repeated\s+)?"
            r"(\w+(?:\.\w+)*)\s+"
            r"(\w+)\s*=\s*(\d+)"
        )
        for fm in field_pattern.finditer(body):
            comment_block = fm.group(1) or ""
            comment_lines = [
                l.lstrip("/").strip()
                for l in comment_block.strip().split("\n")
                if l.strip().startswith("//")
            ]
            modifier = (fm.group(2) or "").strip()
            fields.append(
                {
                    "modifier": modifier,
                    "type": fm.group(3),
                    "name": fm.group(4),
                    "number": int(fm.group(5)),
                    "comment": " ".join(comment_lines),
                }
            )
        result["messages"][msg_name] = fields

    # Extract enums
    enum_pattern = re.compile(r"enum\s+(\w+)\s*\{(.*?)\}", re.DOTALL)
    for m in enum_pattern.finditer(content):
        enum_name = m.group(1)
        body = m.group(2)
        values = re.findall(r"(\w+)\s*=\s*\d+", body)
        result["enums"][enum_name] = values

    return result


PROTO_TYPE_MAP = {
    "string": ("string", None),
    "bool": ("boolean", None),
    "int32": ("integer", "int32"),
    "int64": ("integer", "int64"),
    "uint32": ("integer", "int32"),
    "uint64": ("integer", "int64"),
    "float": ("number", "float"),
    "double": ("number", "double"),
    "bytes": ("string", "byte"),
    "google.protobuf.Timestamp": ("string", "date-time"),
}


def proto_type_to_schema(
    field_type: str, modifier: str, enums: dict, messages: dict
) -> dict:
    """Convert a proto field type to an OpenAPI schema fragment."""
    schema = {}

    if field_type in PROTO_TYPE_MAP:
        t, fmt = PROTO_TYPE_MAP[field_type]
        schema = {"type": t}
        if fmt:
            schema["format"] = fmt
    elif field_type in enums:
        schema = {"type": "string", "enum": enums[field_type]}
    elif field_type in messages:
        schema = {"$ref": f"#/components/schemas/{field_type}"}
    else:
        schema = {"type": "string"}

    if modifier == "repeated":
        schema = {"type": "array", "items": schema}

    return schema


def message_to_schema(
    msg_name: str, fields: list, enums: dict, messages: dict
) -> dict:
    """Convert a proto message to an OpenAPI schema object."""
    if not fields:
        return {"type": "object", "description": f"{msg_name} (empty message)"}

    properties = {}
    required = []
    for f in fields:
        prop = proto_type_to_schema(f["type"], f["modifier"], enums, messages)
        if f["comment"]:
            if "$ref" not in prop:
                prop["description"] = f["comment"]
        if f["modifier"] == "optional":
            if "$ref" not in prop:
                prop["nullable"] = True
        elif f["modifier"] != "repeated":
            required.append(f["name"])
        properties[f["name"]] = prop

    schema = {"type": "object", "properties": properties}
    if required:
        schema["required"] = required
    return schema


def generate_openapi(proto_data: dict, layer_name: str, is_stub: bool) -> str:
    """Generate OpenAPI 3.0 YAML string from parsed proto data."""
    service = proto_data["service_name"]
    description = proto_data["service_comment"] or f"{service} API"
    version = proto_data.get("version", "v1") or "v1"

    # Determine display name for the layer
    display_name = layer_name.capitalize()

    lines = []
    lines.append("# GENERATED FILE — DO NOT EDIT")
    lines.append("# Source: layers/{}/proto/{}.proto".format(layer_name, layer_name))
    lines.append("# Regenerate with: bash scripts/gen-openapi.sh")
    lines.append("openapi: '3.0.3'")
    lines.append("info:")
    lines.append(f"  title: Syfrah {display_name} API")
    lines.append("  version: '1.0'")
    lines.append(f"  description: '{description}'")
    lines.append("servers:")
    lines.append("  - url: https://{gateway}/v1")
    lines.append(f"    description: {display_name} gateway")
    lines.append("    variables:")
    lines.append("      gateway:")
    lines.append("        default: localhost:8443")
    lines.append("security:")
    lines.append("  - BearerApiKey: []")

    # Collect tags from RPCs
    tag_set = OrderedDict()
    for rpc in proto_data["rpcs"]:
        tag = _rpc_tag(rpc["name"], layer_name)
        tag_set[tag] = True

    lines.append("tags:")
    for tag in tag_set:
        lines.append(f"  - name: '{tag}'")

    lines.append("paths:")

    if is_stub:
        path = f"/{version}/{layer_name}/status"
        lines.append(f"  '{path}':")
        lines.append("    get:")
        lines.append(f"      operationId: get{display_name}Status")
        lines.append(f"      summary: Get {layer_name} layer status")
        lines.append(f"      description: Returns 501 — {layer_name} layer is not yet implemented.")
        lines.append(f"      tags:")
        lines.append(f"        - '{display_name}: Status'")
        lines.append("      responses:")
        lines.append("        '501':")
        lines.append("          description: Not Implemented")
        lines.append("          content:")
        lines.append("            application/json:")
        lines.append("              schema:")
        lines.append("                type: object")
        lines.append("                properties:")
        lines.append("                  status:")
        lines.append("                    type: string")
        lines.append("                    example: not_implemented")
        lines.append("                  message:")
        lines.append("                    type: string")
        lines.append(f"                    example: '{layer_name} layer coming soon'")
    else:
        for rpc in proto_data["rpcs"]:
            if not rpc["http_path"]:
                continue
            method = rpc["http_method"].lower()
            path = f"/{version}{rpc['http_path']}"
            tag = _rpc_tag(rpc["name"], layer_name)
            op_id = rpc["name"][0].lower() + rpc["name"][1:]

            lines.append(f"  '{path}':")
            lines.append(f"    {method}:")
            lines.append(f"      operationId: {op_id}")
            lines.append(f"      summary: {_humanize(rpc['name'])}")
            if rpc["comment"]:
                safe_desc = rpc["comment"].replace("'", "''")
                lines.append(f"      description: '{safe_desc}'")
            lines.append(f"      tags:")
            lines.append(f"        - '{tag}'")

            # Request body for POST/PUT/PATCH
            if method in ("post", "put", "patch") and rpc["has_body"]:
                req_msg = rpc["request"]
                req_fields = proto_data["messages"].get(req_msg, [])
                if req_fields:
                    lines.append("      requestBody:")
                    lines.append("        required: true")
                    lines.append("        content:")
                    lines.append("          application/json:")
                    lines.append("            schema:")
                    lines.append(f"              $ref: '#/components/schemas/{req_msg}'")
                else:
                    lines.append("      requestBody:")
                    lines.append("        content:")
                    lines.append("          application/json:")
                    lines.append("            schema:")
                    lines.append("              type: object")

            # Response
            resp_msg = rpc["response"]
            resp_fields = proto_data["messages"].get(resp_msg, [])
            lines.append("      responses:")
            lines.append("        '200':")
            lines.append("          description: Success")
            lines.append("          content:")
            lines.append("            application/json:")
            lines.append("              schema:")
            if resp_fields:
                lines.append(
                    f"                $ref: '#/components/schemas/{resp_msg}'"
                )
            else:
                lines.append("                type: object")
            lines.append("        '401':")
            lines.append("          $ref: '#/components/responses/Unauthorized'")

    # Components
    lines.append("components:")
    lines.append("  securitySchemes:")
    lines.append("    BearerApiKey:")
    lines.append("      type: http")
    lines.append("      scheme: bearer")
    lines.append("      description: 'API key prefixed with syf_key_. Pass as Authorization: Bearer syf_key_...'")
    lines.append("  responses:")
    lines.append("    Unauthorized:")
    lines.append("      description: Missing or invalid API key")
    lines.append("      content:")
    lines.append("        application/json:")
    lines.append("          schema:")
    lines.append("            $ref: '#/components/schemas/Error'")
    lines.append("  schemas:")
    lines.append("    Error:")
    lines.append("      type: object")
    lines.append("      required:")
    lines.append("        - error")
    lines.append("      properties:")
    lines.append("        error:")
    lines.append("          type: string")
    lines.append("          description: Human-readable error message.")

    if not is_stub:
        # Generate schemas for all messages used by RPCs
        used_messages = set()
        for rpc in proto_data["rpcs"]:
            if not rpc["http_path"]:
                continue
            used_messages.add(rpc["request"])
            used_messages.add(rpc["response"])
            # Also add any message types referenced by these messages
            for msg in [rpc["request"], rpc["response"]]:
                for field in proto_data["messages"].get(msg, []):
                    if field["type"] in proto_data["messages"]:
                        used_messages.add(field["type"])

        for msg_name in sorted(used_messages):
            fields = proto_data["messages"].get(msg_name, [])
            schema = message_to_schema(
                msg_name, fields, proto_data["enums"], proto_data["messages"]
            )
            lines.append(f"    {msg_name}:")
            _render_schema(lines, schema, indent=6)

    return "\n".join(lines) + "\n"


def _render_schema(lines: list, schema: dict, indent: int):
    """Recursively render a schema dict as YAML lines."""
    prefix = " " * indent
    if "$ref" in schema:
        lines.append(f"{prefix}$ref: '{schema['$ref']}'")
        return

    for key, value in schema.items():
        if key == "properties":
            lines.append(f"{prefix}properties:")
            for prop_name, prop_schema in value.items():
                lines.append(f"{prefix}  {prop_name}:")
                _render_schema(lines, prop_schema, indent + 4)
        elif key == "items":
            lines.append(f"{prefix}items:")
            _render_schema(lines, value, indent + 2)
        elif key == "required":
            lines.append(f"{prefix}required:")
            for r in value:
                lines.append(f"{prefix}  - {r}")
        elif key == "enum":
            lines.append(f"{prefix}enum:")
            for v in value:
                lines.append(f"{prefix}  - {v}")
        elif isinstance(value, bool):
            lines.append(f"{prefix}{key}: {'true' if value else 'false'}")
        elif isinstance(value, str) and (":" in value or "'" in value or "#" in value):
            safe = value.replace("'", "''")
            lines.append(f"{prefix}{key}: '{safe}'")
        else:
            lines.append(f"{prefix}{key}: {value}")


def _rpc_tag(rpc_name: str, layer_name: str) -> str:
    """Return the layer name as the tag — Scalar groups by tag in the sidebar."""
    return layer_name.capitalize()


def _humanize(name: str) -> str:
    """Convert CamelCase to human-readable string."""
    s = re.sub(r"([A-Z])", r" \1", name).strip()
    return s[0].upper() + s[1:] if s else name


# Stub layers: those whose only RPC is GetStatus
STUB_LAYERS = {"compute", "forge", "overlay", "storage", "org"}


def main():
    repo_root = sys.argv[1] if len(sys.argv) > 1 else "."
    layers_dir = Path(repo_root) / "layers"

    for layer_dir in sorted(layers_dir.iterdir()):
        if not layer_dir.is_dir():
            continue
        layer_name = layer_dir.name
        proto_path = layer_dir / "proto" / f"{layer_name}.proto"
        if not proto_path.exists():
            continue

        print(f"--- Generating OpenAPI for layer: {layer_name} ---")
        proto_data = parse_proto(str(proto_path))

        is_stub = layer_name in STUB_LAYERS
        openapi_yaml = generate_openapi(proto_data, layer_name, is_stub)

        out_path = layer_dir / "openapi.yaml"
        with open(out_path, "w") as f:
            f.write(openapi_yaml)
        print(f"  -> {out_path}")


if __name__ == "__main__":
    main()
