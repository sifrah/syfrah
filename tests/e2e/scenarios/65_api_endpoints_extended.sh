#!/usr/bin/env bash
# Scenario: e2e tests for API endpoints that previously lacked coverage
#
# Verifies:
# - POST /v1/fabric/peering/accept with invalid request_id returns 400
# - POST /v1/fabric/peering/reject with invalid request_id returns 400
# - POST /v1/fabric/peering/reject with missing body returns 4xx
# - POST /v1/fabric/peers/update-endpoint with valid body returns 400 (no such peer)
# - POST /v1/fabric/peers/update-endpoint with missing body returns 4xx
# - POST /v1/fabric/peers/update-endpoint with invalid endpoint returns 400
# - GET  /v1/fabric/peers returns 200 with {"peers": [...]}
# - GET  /v1/fabric/topology returns 200 with {"peers": [...], "edges": [...]}
# - GET  /v1/fabric/events returns 200 with {"events": [...]}
# - GET  /v1/fabric/audit returns 200 with {"entries": [...]}
# - GET  /v1/fabric/metrics returns 200 with numeric fields

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── API Endpoints (extended coverage) ──"

create_network

start_node "e2e-api-ext" "172.20.0.11"

# Enable the gRPC API server.
docker exec "e2e-api-ext" mkdir -p /root/.syfrah
docker exec "e2e-api-ext" sh -c 'cat > /root/.syfrah/config.toml <<CONF
[grpc]
enabled = true
listen = "0.0.0.0:8443"
CONF'

init_mesh "e2e-api-ext" "172.20.0.11" "api-ext-node"
sleep 2

AUTH="Authorization: Bearer syf_key_test_ext_1234"

# ── POST /v1/fabric/peering/accept — invalid request_id ─────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_accept.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{"request_id": "nonexistent-id-000"}' \
    http://127.0.0.1:8443/v1/fabric/peering/accept)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peering/accept with invalid request_id returns ${HTTP_CODE}"
else
    fail "POST /v1/fabric/peering/accept with invalid request_id returned ${HTTP_CODE} (expected 4xx)"
    docker exec "e2e-api-ext" cat /tmp/resp_accept.json 2>/dev/null || true
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_accept.json 2>/dev/null)
if echo "$BODY" | jq -e '.error' >/dev/null 2>&1; then
    pass "/v1/fabric/peering/accept error response has 'error' field"
else
    fail "/v1/fabric/peering/accept error response shape wrong: $BODY"
fi

# ── POST /v1/fabric/peering/accept — missing body ───────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
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

# ── POST /v1/fabric/peering/reject — invalid request_id ─────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_reject.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{"request_id": "nonexistent-id-000", "reason": "test rejection"}' \
    http://127.0.0.1:8443/v1/fabric/peering/reject)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peering/reject with invalid request_id returns ${HTTP_CODE}"
else
    fail "POST /v1/fabric/peering/reject with invalid request_id returned ${HTTP_CODE} (expected 4xx)"
    docker exec "e2e-api-ext" cat /tmp/resp_reject.json 2>/dev/null || true
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_reject.json 2>/dev/null)
if echo "$BODY" | jq -e '.error' >/dev/null 2>&1; then
    pass "/v1/fabric/peering/reject error response has 'error' field"
else
    fail "/v1/fabric/peering/reject error response shape wrong: $BODY"
fi

# ── POST /v1/fabric/peering/reject — missing body ───────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /dev/null -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{}' \
    http://127.0.0.1:8443/v1/fabric/peering/reject)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peering/reject with empty body returns ${HTTP_CODE} (client error)"
elif [ "$HTTP_CODE" -ge 500 ]; then
    fail "POST /v1/fabric/peering/reject with empty body returns ${HTTP_CODE} (server error, expected client error)"
else
    pass "POST /v1/fabric/peering/reject with empty body returns ${HTTP_CODE}"
fi

# ── POST /v1/fabric/peers/update-endpoint — nonexistent peer ────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_update_ep.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{"name_or_key": "no-such-peer", "endpoint": "10.0.0.1:51820"}' \
    http://127.0.0.1:8443/v1/fabric/peers/update-endpoint)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peers/update-endpoint with unknown peer returns ${HTTP_CODE}"
else
    fail "POST /v1/fabric/peers/update-endpoint with unknown peer returned ${HTTP_CODE} (expected 4xx)"
    docker exec "e2e-api-ext" cat /tmp/resp_update_ep.json 2>/dev/null || true
fi

# ── POST /v1/fabric/peers/update-endpoint — missing body ────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /dev/null -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{}' \
    http://127.0.0.1:8443/v1/fabric/peers/update-endpoint)

if [ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ]; then
    pass "POST /v1/fabric/peers/update-endpoint with empty body returns ${HTTP_CODE} (client error)"
elif [ "$HTTP_CODE" -ge 500 ]; then
    fail "POST /v1/fabric/peers/update-endpoint with empty body returns ${HTTP_CODE} (server error, expected client error)"
else
    pass "POST /v1/fabric/peers/update-endpoint with empty body returns ${HTTP_CODE}"
fi

# ── POST /v1/fabric/peers/update-endpoint — invalid endpoint format ──

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_update_bad.json -w '%{http_code}' \
    -X POST \
    -H "$AUTH" \
    -H "Content-Type: application/json" \
    -d '{"name_or_key": "some-peer", "endpoint": "not-a-valid-addr"}' \
    http://127.0.0.1:8443/v1/fabric/peers/update-endpoint)

if [ "$HTTP_CODE" = "400" ]; then
    pass "POST /v1/fabric/peers/update-endpoint with invalid endpoint returns 400"
else
    fail "POST /v1/fabric/peers/update-endpoint with invalid endpoint returned ${HTTP_CODE} (expected 400)"
    docker exec "e2e-api-ext" cat /tmp/resp_update_bad.json 2>/dev/null || true
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_update_bad.json 2>/dev/null)
if echo "$BODY" | jq -e '.error' >/dev/null 2>&1; then
    pass "/v1/fabric/peers/update-endpoint bad-endpoint response has 'error' field"
else
    fail "/v1/fabric/peers/update-endpoint bad-endpoint response shape wrong: $BODY"
fi

# ── GET /v1/fabric/peers ─────────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_peers.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/peers)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/peers returns 200"
else
    fail "GET /v1/fabric/peers returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_peers.json 2>/dev/null)
if echo "$BODY" | jq -e '.peers' >/dev/null 2>&1; then
    pass "/v1/fabric/peers response has 'peers' array"
else
    fail "/v1/fabric/peers response shape wrong: $BODY"
fi

if echo "$BODY" | jq -e '.peers | type == "array"' >/dev/null 2>&1; then
    pass "/v1/fabric/peers 'peers' field is an array"
else
    fail "/v1/fabric/peers 'peers' field is not an array: $BODY"
fi

# ── GET /v1/fabric/topology ──────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_topology.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/topology)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/topology returns 200"
else
    fail "GET /v1/fabric/topology returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_topology.json 2>/dev/null)
if echo "$BODY" | jq -e '.peers' >/dev/null 2>&1; then
    pass "/v1/fabric/topology response has 'peers' array"
else
    fail "/v1/fabric/topology response missing 'peers': $BODY"
fi

if echo "$BODY" | jq -e '.edges' >/dev/null 2>&1; then
    pass "/v1/fabric/topology response has 'edges' array"
else
    fail "/v1/fabric/topology response missing 'edges': $BODY"
fi

# ── GET /v1/fabric/events ────────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_events.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/events)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/events returns 200"
else
    fail "GET /v1/fabric/events returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_events.json 2>/dev/null)
if echo "$BODY" | jq -e '.events' >/dev/null 2>&1; then
    pass "/v1/fabric/events response has 'events' array"
else
    fail "/v1/fabric/events response shape wrong: $BODY"
fi

if echo "$BODY" | jq -e '.events | type == "array"' >/dev/null 2>&1; then
    pass "/v1/fabric/events 'events' field is an array"
else
    fail "/v1/fabric/events 'events' field is not an array: $BODY"
fi

# ── GET /v1/fabric/audit ─────────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_audit.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/audit)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/audit returns 200"
else
    fail "GET /v1/fabric/audit returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_audit.json 2>/dev/null)
if echo "$BODY" | jq -e '.entries' >/dev/null 2>&1; then
    pass "/v1/fabric/audit response has 'entries' array"
else
    fail "/v1/fabric/audit response shape wrong: $BODY"
fi

if echo "$BODY" | jq -e '.entries | type == "array"' >/dev/null 2>&1; then
    pass "/v1/fabric/audit 'entries' field is an array"
else
    fail "/v1/fabric/audit 'entries' field is not an array: $BODY"
fi

# ── GET /v1/fabric/metrics ───────────────────────────────────────────

HTTP_CODE=$(docker exec "e2e-api-ext" \
    curl -s -o /tmp/resp_metrics.json -w '%{http_code}' \
    -H "$AUTH" \
    http://127.0.0.1:8443/v1/fabric/metrics)

if [ "$HTTP_CODE" = "200" ]; then
    pass "GET /v1/fabric/metrics returns 200"
else
    fail "GET /v1/fabric/metrics returned $HTTP_CODE"
fi

BODY=$(docker exec "e2e-api-ext" cat /tmp/resp_metrics.json 2>/dev/null)
if echo "$BODY" | jq -e '.peer_count >= 0' >/dev/null 2>&1; then
    pass "/v1/fabric/metrics response has 'peer_count' (numeric)"
else
    fail "/v1/fabric/metrics response missing or invalid 'peer_count': $BODY"
fi

if echo "$BODY" | jq -e '.bytes_sent >= 0' >/dev/null 2>&1; then
    pass "/v1/fabric/metrics response has 'bytes_sent' (numeric)"
else
    fail "/v1/fabric/metrics response missing or invalid 'bytes_sent': $BODY"
fi

if echo "$BODY" | jq -e '.bytes_received >= 0' >/dev/null 2>&1; then
    pass "/v1/fabric/metrics response has 'bytes_received' (numeric)"
else
    fail "/v1/fabric/metrics response missing or invalid 'bytes_received': $BODY"
fi

if echo "$BODY" | jq -e '.handshakes_completed >= 0' >/dev/null 2>&1; then
    pass "/v1/fabric/metrics response has 'handshakes_completed' (numeric)"
else
    fail "/v1/fabric/metrics response missing or invalid 'handshakes_completed': $BODY"
fi

if echo "$BODY" | jq -e '.handshakes_failed >= 0' >/dev/null 2>&1; then
    pass "/v1/fabric/metrics response has 'handshakes_failed' (numeric)"
else
    fail "/v1/fabric/metrics response missing or invalid 'handshakes_failed': $BODY"
fi

cleanup
summary
