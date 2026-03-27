#!/usr/bin/env bash
# Scenario: invalid API key returns 401
#
# Verifies:
# - A request with a wrong/invalid Bearer token is rejected with HTTP 401
# - The error response includes an "error" field and a "trace_id"

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── API Auth: Invalid Key ──"

create_network

start_node "e2e-api-auth-invalid" "172.20.0.10"

# Enable the gRPC API server.
docker exec "e2e-api-auth-invalid" mkdir -p /root/.syfrah
docker exec "e2e-api-auth-invalid" sh -c 'cat > /root/.syfrah/config.toml <<CONF
[grpc]
enabled = true
listen = "0.0.0.0:8443"
CONF'

init_mesh "e2e-api-auth-invalid" "172.20.0.10" "api-node"
sleep 2

# Call with a key that does not have the syf_key_ prefix.
HTTP_CODE=$(docker exec "e2e-api-auth-invalid" \
    curl -s -o /tmp/api_resp.json -w '%{http_code}' \
    -H "Authorization: Bearer bad_prefix_key_xxxx" \
    http://127.0.0.1:8443/v1/fabric/status)

if [ "$HTTP_CODE" = "401" ]; then
    pass "wrong-prefix key returns HTTP 401"
else
    fail "expected HTTP 401, got $HTTP_CODE"
    docker exec "e2e-api-auth-invalid" cat /tmp/api_resp.json 2>/dev/null || true
fi

# Call with a syf_key_ that is too short (invalid).
HTTP_CODE2=$(docker exec "e2e-api-auth-invalid" \
    curl -s -o /tmp/api_resp2.json -w '%{http_code}' \
    -H "Authorization: Bearer syf_key_" \
    http://127.0.0.1:8443/v1/fabric/status)

if [ "$HTTP_CODE2" = "401" ]; then
    pass "short key returns HTTP 401"
else
    fail "expected HTTP 401 for short key, got $HTTP_CODE2"
fi

# Verify the error response contains expected fields.
BODY=$(docker exec "e2e-api-auth-invalid" cat /tmp/api_resp.json 2>/dev/null)
if echo "$BODY" | jq -e '.error' >/dev/null 2>&1; then
    pass "error response contains 'error' field"
else
    fail "error response missing 'error' field: $BODY"
fi

if echo "$BODY" | jq -e '.trace_id' >/dev/null 2>&1; then
    pass "error response contains 'trace_id' field"
else
    fail "error response missing 'trace_id' field: $BODY"
fi

cleanup
summary
