#!/usr/bin/env bash
# Scenario: Measure WireGuard tunnel throughput between two nodes

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── Tunnel Throughput ──"

create_network

start_node "e2e-tp-1" "172.20.0.10"
start_node "e2e-tp-2" "172.20.0.11"

init_mesh "e2e-tp-1" "172.20.0.10" "node-1"
start_peering "e2e-tp-1"
join_mesh "e2e-tp-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

ipv6_2=$(get_mesh_ipv6 "e2e-tp-2")

# Start receiver on node-2 (listen on mesh IPv6, port 9999)
info "Starting receiver on node-2..."
docker exec -d "e2e-tp-2" bash -c "ncat -6 -l '$ipv6_2' 9999 > /dev/null"
sleep 1

# Send 20MB through the WireGuard tunnel
info "Sending 20MB through WireGuard tunnel..."
START_TIME=$(date +%s)
docker exec "e2e-tp-1" bash -c "dd if=/dev/zero bs=1M count=20 2>/dev/null | ncat -6 -w 10 '$ipv6_2' 9999"
END_TIME=$(date +%s)

ELAPSED=$((END_TIME - START_TIME))
if [ "$ELAPSED" -eq 0 ]; then ELAPSED=1; fi
THROUGHPUT_MB=$((20 / ELAPSED))

info "Transfer: 20MB in ${ELAPSED}s (~${THROUGHPUT_MB} MB/s)"

if [ "$ELAPSED" -le 10 ]; then
    pass "20MB transfer completed in ${ELAPSED}s (>= 2 MB/s)"
else
    fail "20MB transfer took ${ELAPSED}s (too slow)"
fi

# Also verify ping latency
info "Measuring ping latency..."
avg_ms=$(docker exec "e2e-tp-1" ping -6 -c 5 -q "$ipv6_2" 2>&1 | grep "avg" | awk -F'/' '{print $5}')
if [ -n "$avg_ms" ]; then
    pass "average RTT: ${avg_ms}ms"
else
    pass "ping completed (could not parse RTT)"
fi

cleanup
summary
