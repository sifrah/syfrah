#!/usr/bin/env bash
set -uo pipefail

PROTO_DIR="$(cd "$(dirname "$0")/../api/proto/syfrah/v1" && pwd)"

for proto in "$PROTO_DIR"/*.proto; do
    name=$(basename "$proto" .proto)
    out="$PROTO_DIR/$name.md"
    package=$(grep "^package " "$proto" 2>/dev/null | sed 's/package //;s/;//' || echo "")
    service=$(grep "^service " "$proto" 2>/dev/null | sed 's/service //;s/ {//' || echo "")

    {
        echo "# ${name^} API Reference"
        echo ""
        echo "Package: \`$package\`"
        echo ""
        
        if [ -n "$service" ]; then
            echo "## Service: $service"
            echo ""
            echo "| Method | Request | Response |"
            echo "|--------|---------|----------|"
            grep "  rpc " "$proto" 2>/dev/null | while read -r line; do
                method=$(echo "$line" | sed 's/.*rpc \([A-Za-z]*\).*/\1/')
                req=$(echo "$line" | sed 's/.*(\([A-Za-z]*\)).*/\1/')
                resp=$(echo "$line" | sed 's/.*returns (\([A-Za-z]*\)).*/\1/')
                echo "| \`$method\` | \`$req\` | \`$resp\` |"
            done
            echo ""
        fi
        
        echo "## Messages"
        echo ""
        
        in_msg=false
        while IFS= read -r line; do
            if [[ "$line" =~ ^message\  ]]; then
                msg=$(echo "$line" | sed 's/message //;s/ {//')
                echo "### $msg"
                echo ""
                echo "| Field | Type | Number |"
                echo "|-------|------|--------|"
                in_msg=true
            elif [[ "$line" =~ ^"}" ]] && $in_msg; then
                echo ""
                in_msg=false
            elif $in_msg && [[ "$line" =~ "=" ]]; then
                ft=$(echo "$line" | sed 's/^ *//' | awk '{print $1}')
                fn=$(echo "$line" | sed 's/^ *//' | awk '{print $2}')
                fnum=$(echo "$line" | grep -oP '= \K\d+' || echo "?")
                [ -n "$fn" ] && echo "| \`$fn\` | \`$ft\` | $fnum |"
            fi
        done < "$proto"
        
        echo "---"
        echo ""
        echo "Source: \`api/proto/syfrah/v1/$name.proto\`"
    } > "$out"
    
    echo "  Generated $name.md"
done
