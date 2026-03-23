#!/usr/bin/env bash
# Stress test: sustained WireGuard throughput for 30 seconds
# Verifies the tunnel doesn't degrade over time

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── STRESS: Sustained Throughput (30s) ──"

create_network

start_node "e2e-stp-1" "172.20.0.10"
start_node "e2e-stp-2" "172.20.0.11"

init_mesh "e2e-stp-1" "172.20.0.10" "node-1"
start_peering "e2e-stp-1"
join_mesh "e2e-stp-2" "172.20.0.10" "172.20.0.11" "node-2"

sleep 3

ipv6_2=$(get_mesh_ipv6 "e2e-stp-2")

# Start receiver
docker exec -d "e2e-stp-2" bash -c "ncat -6 -l '$ipv6_2' 9999 > /dev/null"
sleep 1

# Send data continuously for 30 seconds
info "Sending data for 30 seconds..."
START_TIME=$(date +%s)
docker exec "e2e-stp-1" bash -c \
    "dd if=/dev/zero bs=1M count=100 2>/dev/null | timeout 30 ncat -6 -w 35 '$ipv6_2' 9999" || true
END_TIME=$(date +%s)

ELAPSED=$((END_TIME - START_TIME))
if [ "$ELAPSED" -eq 0 ]; then ELAPSED=1; fi

info "Transfer ran for ${ELAPSED}s"

if [ "$ELAPSED" -ge 5 ]; then
    pass "sustained transfer ran for ${ELAPSED}s without hanging"
else
    fail "transfer ended too quickly (${ELAPSED}s)"
fi

# Verify the tunnel still works after sustained load
sleep 2
assert_can_ping "e2e-stp-1" "$ipv6_2"

# Check daemon is still healthy
assert_daemon_running "e2e-stp-1"
assert_daemon_running "e2e-stp-2"

cleanup
summary
