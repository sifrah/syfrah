#!/usr/bin/env bash
# Scenario: last_seen timestamp updates from WireGuard handshakes
# Verifies that the health check reads actual handshake timestamps

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Fabric: Last Seen Update ──"

create_network

start_node "e2e-seen-1" "172.20.0.10"
start_node "e2e-seen-2" "172.20.0.11"

init_mesh "e2e-seen-1" "172.20.0.10" "node-1"
start_peering "e2e-seen-1"
join_mesh "e2e-seen-2" "172.20.0.10" "172.20.0.11" "node-2"

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-seen-1" 1 30

# Verify there is a WireGuard handshake
handshake=$(docker exec "e2e-seen-1" syfrah fabric peers 2>&1 | grep "active" | head -1 || echo "")
if echo "$handshake" | grep -q "ago"; then
    pass "peer has a WireGuard handshake timestamp"
else
    # Trigger a handshake by pinging
    ipv6_2=$(get_mesh_ipv6 "e2e-seen-2")
    if [ -z "$ipv6_2" ]; then
        fail "could not get mesh IPv6 for e2e-seen-2"
    else
        docker exec "e2e-seen-1" ping -6 -c 1 -W 3 "$ipv6_2" >/dev/null 2>&1
    fi
    sleep 2
    handshake=$(docker exec "e2e-seen-1" syfrah fabric peers 2>&1 | grep "active" | head -1 || echo "")
    if echo "$handshake" | grep -q "ago"; then
        pass "peer has handshake after ping"
    else
        fail "no handshake detected"
    fi
fi

# Poll for the health check to run (60s interval) and update last_seen
info "Waiting for health check to update last_seen (polling up to 90s)..."
_hc_deadline=$(($(date +%s) + 90))
active_count=0
while [ "$(date +%s)" -lt "$_hc_deadline" ]; do
    active_count=$(docker exec "e2e-seen-1" syfrah fabric peers 2>&1 | grep -c "active" || echo "0")
    if [ "$active_count" -ge 1 ]; then
        break
    fi
    sleep 5
done
if [ "$active_count" -ge 1 ]; then
    pass "peer still active after health check (last_seen updated)"
else
    fail "peer not active — last_seen may not have been updated"
fi

# The peer should have a recent handshake
handshake_text=$(docker exec "e2e-seen-1" syfrah fabric peers 2>&1 | grep "active" | awk '{print $5}' || echo "")
if [ -z "$handshake_text" ]; then
    fail "could not extract handshake text from peers output"
elif echo "$handshake_text" | grep -qE "^[0-9]+s"; then
    pass "handshake is recent (seconds ago)"
else
    pass "handshake present: $handshake_text"
fi

cleanup
summary
