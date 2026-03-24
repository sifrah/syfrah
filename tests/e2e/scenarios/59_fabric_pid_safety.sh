#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
source "$SCRIPT_DIR/lib.sh"

echo "── PID Safety ──"
create_network
start_node "e2e-pid-1" "172.20.0.10"
init_mesh "e2e-pid-1" "172.20.0.10" "node-1"
wait_daemon "e2e-pid-1" 30

# Write fake PID (PID 1 = init, should NOT be killed)
docker exec "e2e-pid-1" syfrah fabric stop 2>/dev/null || true
sleep 3
docker exec "e2e-pid-1" bash -c 'echo 1 > /root/.syfrah/daemon.pid'

# Try to stop — should refuse to kill PID 1
err=$(docker exec "e2e-pid-1" syfrah fabric stop 2>&1 || true)
echo "$err" | grep -qi "not.*syfrah\|invalid\|refuse\|not running" && \
    pass "refuses to kill non-syfrah PID" || \
    fail "did not refuse to kill fake PID"

# PID 1 should still be alive
docker exec "e2e-pid-1" kill -0 1 2>/dev/null && \
    pass "PID 1 (init) still alive" || fail "PID 1 was killed!"

cleanup
summary
