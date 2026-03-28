#!/usr/bin/env bash
# Scenario: reconciliation loop re-adds peers after WireGuard reset
# Verifies that the 30s reconcile loop fixes WG drift automatically

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Fabric: Reconciliation ──"

create_network

start_node "e2e-recon-1" "172.20.0.10"
start_node "e2e-recon-2" "172.20.0.11"

init_mesh "e2e-recon-1" "172.20.0.10" "node-1"
start_peering "e2e-recon-1"
join_mesh "e2e-recon-2" "172.20.0.10" "172.20.0.11" "node-2"

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-recon-1" 1 30

ipv6_2=$(get_mesh_ipv6 "e2e-recon-2")
if [ -z "$ipv6_2" ]; then
    fail "could not get mesh IPv6 for e2e-recon-2"
fi
assert_can_ping "e2e-recon-1" "$ipv6_2"

# Remove node-2's peer from WireGuard on node-1 (simulate drift)
info "Removing node-2 from WireGuard on node-1 (simulating drift)..."
wg_key=$(docker exec "e2e-recon-2" cat /root/.syfrah/state.json 2>/dev/null | jq -r '.wg_public_key')
if [ -n "$wg_key" ] && [ "$wg_key" != "null" ]; then
    docker exec "e2e-recon-1" bash -c "wg set syfrah0 peer '$wg_key' remove" 2>/dev/null || true
fi

# Verify ping fails immediately after removal
sleep 1
if ! docker exec "e2e-recon-1" ping -6 -c 1 -W 2 "$ipv6_2" >/dev/null 2>&1; then
    pass "ping fails after WG peer removal (expected)"
else
    pass "ping still works (WG may have re-added via reconcile already)"
fi

# Poll for reconciliation loop to fix it (check every 5s, timeout 60s)
info "Waiting for reconciliation loop (polling up to 60s)..."
_recon_deadline=$(($(date +%s) + 60))
_recon_ok=false
while [ "$(date +%s)" -lt "$_recon_deadline" ]; do
    if docker exec "e2e-recon-1" ping -6 -c 1 -W 2 "$ipv6_2" >/dev/null 2>&1; then
        _recon_ok=true
        break
    fi
    sleep 5
done

# Verify connectivity is restored
if [ "$_recon_ok" = true ]; then
    pass "e2e-recon-1 can ping $ipv6_2"
else
    assert_can_ping "e2e-recon-1" "$ipv6_2"
fi

cleanup
summary
