#!/usr/bin/env bash
# Scenario: valid API key returns 200 on /v1/fabric/status
#
# Verifies:
# - A node with the gRPC API enabled accepts a valid syf_key_ Bearer token
# - /v1/fabric/status returns HTTP 200 with a JSON body

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── API Auth: Valid Key ──"

create_network

start_node "e2e-api-auth-valid" "172.20.0.10"

# Write config.toml to enable the gRPC API server.
docker exec "e2e-api-auth-valid" mkdir -p /root/.syfrah
docker exec "e2e-api-auth-valid" sh -c 'cat > /root/.syfrah/config.toml <<CONF
[grpc]
enabled = true
listen = "0.0.0.0:8443"
CONF'

init_mesh "e2e-api-auth-valid" "172.20.0.10" "api-node"

# Give the gRPC API server a moment to bind.
sleep 2

# Call /v1/fabric/status with a valid API key.
HTTP_CODE=$(docker exec "e2e-api-auth-valid" \
    curl -s -o /tmp/api_resp.json -w '%{http_code}' \
    -H "Authorization: Bearer syf_key_test_valid_1234" \
    http://127.0.0.1:8443/v1/fabric/status)

if [ "$HTTP_CODE" = "200" ]; then
    pass "valid key returns HTTP 200"
else
    fail "expected HTTP 200, got $HTTP_CODE"
    docker exec "e2e-api-auth-valid" cat /tmp/api_resp.json 2>/dev/null || true
fi

# Verify the response is valid JSON with a "status" field.
BODY=$(docker exec "e2e-api-auth-valid" cat /tmp/api_resp.json 2>/dev/null)
if echo "$BODY" | jq -e '.status' >/dev/null 2>&1; then
    pass "response contains 'status' field"
else
    fail "response missing 'status' field: $BODY"
fi

cleanup
summary
