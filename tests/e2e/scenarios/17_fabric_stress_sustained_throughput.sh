#!/usr/bin/env bash
# Stress test: large data transfer via WireGuard tunnel
# Verifies the tunnel handles bulk data without corruption or stalls

SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── STRESS: Bulk Throughput ──"

create_network

start_node "e2e-stp-1" "172.20.0.10"
start_node "e2e-stp-2" "172.20.0.11"

init_mesh "e2e-stp-1" "172.20.0.10" "node-1"
start_peering "e2e-stp-1"
join_mesh "e2e-stp-2" "172.20.0.10" "172.20.0.11" "node-2"

# Wait for peer convergence instead of fixed sleep
wait_for_peer_active "e2e-stp-1" 1 30

ipv6_2=$(get_mesh_ipv6 "e2e-stp-2")
if [ -z "$ipv6_2" ]; then
    fail "could not get mesh IPv6 for e2e-stp-2"
fi

# Transfer 1: 100MB bulk transfer
info "Transfer 1: 100MB bulk..."
docker exec -d "e2e-stp-2" bash -c "ncat -6 -l '$ipv6_2' 9999 > /tmp/received"
sleep 2

START_TIME=$(date +%s)
docker exec "e2e-stp-1" bash -c \
    "dd if=/dev/zero bs=1M count=100 2>/dev/null | ncat -6 -w 30 '$ipv6_2' 9999"
END_TIME=$(date +%s)
ELAPSED=$((END_TIME - START_TIME))
if [ "$ELAPSED" -eq 0 ]; then ELAPSED=1; fi

THROUGHPUT=$((100 / ELAPSED))
info "100MB in ${ELAPSED}s (~${THROUGHPUT} MB/s)"

if [ "$ELAPSED" -le 30 ]; then
    pass "100MB transfer completed in ${ELAPSED}s"
else
    fail "100MB transfer took ${ELAPSED}s (too slow)"
fi

# Verify received size
RECEIVED=$(docker exec "e2e-stp-2" wc -c /tmp/received 2>/dev/null | awk '{print $1}' || echo "0")
RECEIVED=${RECEIVED:-0}
EXPECTED=$((100 * 1024 * 1024))
if [ "$RECEIVED" = "$EXPECTED" ]; then
    pass "receiver got all 100MB ($RECEIVED bytes)"
else
    fail "receiver got $RECEIVED bytes (expected $EXPECTED)"
fi

sleep 1

# Transfer 2: multiple small transfers (tests connection reuse)
info "Transfer 2: 10x 1MB rapid transfers..."
FAIL_COUNT=0
for round in $(seq 1 10); do
    docker exec -d "e2e-stp-2" bash -c "ncat -6 -l '$ipv6_2' 9998 > /dev/null"
    sleep 0.3
    if docker exec "e2e-stp-1" bash -c \
        "dd if=/dev/zero bs=1M count=1 2>/dev/null | ncat -6 -w 5 '$ipv6_2' 9998"; then
        true
    else
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
done

if [ "$FAIL_COUNT" -eq 0 ]; then
    pass "10 rapid transfers all succeeded"
else
    fail "$FAIL_COUNT/10 rapid transfers failed"
fi

# Tunnel still works after all the load
assert_can_ping "e2e-stp-1" "$ipv6_2"
assert_daemon_running "e2e-stp-1"

cleanup
summary
