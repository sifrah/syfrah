#!/usr/bin/env bash
# Scenario: missing Authorization header returns 401
#
# Verifies:
# - A request without any Authorization header is rejected with HTTP 401
# - The error message mentions "missing"

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── API Auth: No Key ──"

create_network

start_node "e2e-api-auth-nokey" "172.20.0.10"

# Enable the gRPC API server.
docker exec "e2e-api-auth-nokey" mkdir -p /root/.syfrah
docker exec "e2e-api-auth-nokey" sh -c 'cat > /root/.syfrah/config.toml <<CONF
[grpc]
enabled = true
listen = "0.0.0.0:8443"
CONF'

init_mesh "e2e-api-auth-nokey" "172.20.0.10" "api-node"
sleep 2

# Call /v1/fabric/status with NO Authorization header.
HTTP_CODE=$(docker exec "e2e-api-auth-nokey" \
    curl -s -o /tmp/api_resp.json -w '%{http_code}' \
    http://127.0.0.1:8443/v1/fabric/status)

if [ "$HTTP_CODE" = "401" ]; then
    pass "no-auth request returns HTTP 401"
else
    fail "expected HTTP 401, got $HTTP_CODE"
    docker exec "e2e-api-auth-nokey" cat /tmp/api_resp.json 2>/dev/null || true
fi

# Verify the error message mentions "missing".
BODY=$(docker exec "e2e-api-auth-nokey" cat /tmp/api_resp.json 2>/dev/null)
if echo "$BODY" | jq -r '.error' 2>/dev/null | grep -qi "missing"; then
    pass "error mentions 'missing' Authorization header"
else
    fail "error should mention 'missing', got: $BODY"
fi

# Also verify trace_id is present even on auth failures.
if echo "$BODY" | jq -e '.trace_id' >/dev/null 2>&1; then
    pass "error response contains 'trace_id'"
else
    fail "error response missing 'trace_id': $BODY"
fi

cleanup
summary
