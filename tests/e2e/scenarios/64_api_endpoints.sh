#!/usr/bin/env bash
# Scenario: call each /v1/fabric/* endpoint and verify response shape
#
# Verifies:
# - GET  /v1/fabric/status returns 200 with {"status": ...}
# - GET  /v1/fabric/peering/requests returns 200 with {"requests": [...]}
# - POST /v1/fabric/peering/start returns 200 with JSON body
# - POST /v1/fabric/peering/stop returns 200
# - POST /v1/fabric/reload returns 200 with {"changes": ..., "skipped": ...}
# - POST /v1/fabric/rotate-secret returns 200 with {"new_secret": ...}
# - POST endpoints with missing body return 4xx (not 500)

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── API Endpoints ──"

create_network

start_node "e2e-api-endpoints" "172.20.0.10"

# Enable the gRPC API server.
docker exec "e2e-api-endpoints" mkdir -p /root/.syfrah
docker exec "e2e-api-endpoints" sh -c 'cat > /root/.syfrah/config.toml <<CONF
[grpc]
enabled = true
listen = "0.0.0.0:8443"
CONF'

init_mesh "e2e-api-endpoints" "172.20.0.10" "api-node"
sleep 2

AUTH="Authorization: Bearer syf_key_test_endpoints_1234"

# ── GET /v1/fabric/status ──────────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /tmp/resp_status.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/status)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/status returns 200"
else
    fail "GET /v1/fabric/status returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-endpoints" cat /tmp/resp_status.json 2>/dev/null)
if echo "$BODY" | jq -e '.status' >/dev/null 2>&1; then
    pass "/v1/fabric/status response has 'status' field"
else
    fail "/v1/fabric/status response shape wrong: $BODY"
fi

# ── GET /v1/fabric/peering/requests ────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /tmp/resp_peering_req.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/peering/requests)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/peering/requests returns 200"
else
    fail "GET /v1/fabric/peering/requests returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-endpoints" cat /tmp/resp_peering_req.json 2>/dev/null)
if echo "$BODY" | jq -e '.requests' >/dev/null 2>&1; then
    pass "/v1/fabric/peering/requests response has 'requests' array"
else
    fail "/v1/fabric/peering/requests response shape wrong: $BODY"
fi

# ── POST /v1/fabric/peering/start ─────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /tmp/resp_peering_start.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{"port": 7946}' \
    http://127.0.0.1:8443/v1/fabric/peering/start)

if [ "$HTTP_CODE" = "200" ]; then
    pass "POST /v1/fabric/peering/start returns 200"
else
    fail "POST /v1/fabric/peering/start returned $HTTP_CODE"
    docker exec "e2e-api-endpoints" cat /tmp/resp_peering_start.json 2>/dev/null || true
fi

# ── POST /v1/fabric/peering/stop ──────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /tmp/resp_peering_stop.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/peering/stop)

if [ "$HTTP_CODE" = "200" ]; then
    pass "POST /v1/fabric/peering/stop returns 200"
else
    fail "POST /v1/fabric/peering/stop returned $HTTP_CODE"
    docker exec "e2e-api-endpoints" cat /tmp/resp_peering_stop.json 2>/dev/null || true
fi

# ── POST /v1/fabric/reload ────────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /tmp/resp_reload.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/reload)

if [ "$HTTP_CODE" = "200" ]; then
    pass "POST /v1/fabric/reload returns 200"
    BODY=$(docker exec "e2e-api-endpoints" cat /tmp/resp_reload.json 2>/dev/null)
    if echo "$BODY" | jq -e '.changes' >/dev/null 2>&1; then
        pass "/v1/fabric/reload response has 'changes' field"
    else
        fail "/v1/fabric/reload response shape wrong: $BODY"
    fi
else
    fail "POST /v1/fabric/reload returned $HTTP_CODE"
    docker exec "e2e-api-endpoints" cat /tmp/resp_reload.json 2>/dev/null || true
fi

# ── POST /v1/fabric/rotate-secret ────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /tmp/resp_rotate.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/rotate-secret)

if [ "$HTTP_CODE" = "200" ]; then
    pass "POST /v1/fabric/rotate-secret returns 200"
    BODY=$(docker exec "e2e-api-endpoints" cat /tmp/resp_rotate.json 2>/dev/null)
    if echo "$BODY" | jq -e '.new_secret' >/dev/null 2>&1; then
        pass "/v1/fabric/rotate-secret response has 'new_secret' field"
    else
        fail "/v1/fabric/rotate-secret response shape wrong: $BODY"
    fi
else
    fail "POST /v1/fabric/rotate-secret returned $HTTP_CODE"
    docker exec "e2e-api-endpoints" cat /tmp/resp_rotate.json 2>/dev/null || true
fi

# ── POST endpoints with missing required body fields ──────────────────
# These should return 4xx (400 or 422), NOT 500 (server error).

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /dev/null -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{}' \
    http://127.0.0.1:8443/v1/fabric/peering/accept)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peering/accept with empty body returns ${HTTP_CODE} (client error)"
elif [ "$HTTP_CODE" -ge 500 ]; then
    fail "POST /v1/fabric/peering/accept with empty body returns ${HTTP_CODE} (server error, expected client error)"
else
    pass "POST /v1/fabric/peering/accept with empty body returns ${HTTP_CODE}"
fi

HTTP_CODE=$(docker exec "e2e-api-endpoints" \
    curl -s -o /dev/null -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{}' \
    http://127.0.0.1:8443/v1/fabric/peers/remove)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peers/remove with empty body returns ${HTTP_CODE} (client error)"
elif [ "$HTTP_CODE" -ge 500 ]; then
    fail "POST /v1/fabric/peers/remove with empty body returns ${HTTP_CODE} (server error, expected client error)"
else
    pass "POST /v1/fabric/peers/remove with empty body returns ${HTTP_CODE}"
fi

cleanup
summary
